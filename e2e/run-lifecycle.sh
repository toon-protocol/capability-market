#!/usr/bin/env bash
# Devnet e2e lifecycle runner (toon-meta#84 / #119 / #120).
#
# Drives THREE markets end-to-end against the live TOON devnet with real USDC:
#   A. marketMock — resolved YES via a dev-mode (fake-seal) proof
#   B. marketReal — resolved YES via a REAL Groth16 proof verified on-chain
#   C. marketMock — resolved NO via settleTimeout (keeper bounty path)
#
# Shared-anvil rule: NO time cheatcodes. All windows are short REAL-TIME
# windows and the script simply waits for them to elapse (~6-8 min total,
# plus Groth16 proving on the first run).
#
# Prereqs:
#   - foundry (cast) on PATH, node, docker (for Groth16 proving), rust toolchain
#   - e2e/.env.devnet-actors (gitignored) with <ROLE>_ADDR/<ROLE>_KEY for
#     CREATOR, YESSTAKER, NOSTAKER, MINER, KEEPER — generate with
#     `cast wallet new` and fund each from the devnet faucet:
#       curl -X POST https://faucet.devnet.toonprotocol.dev/api/request \
#         -H 'content-type: application/json' -d '{"address":"0x..."}'
#
# Outputs: e2e/out/results.env (structured run record), proof JSONs in e2e/out/.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v cast >/dev/null 2>&1 || export PATH="$HOME/.foundry/bin:$PATH"

RPC=${DEVNET_RPC_URL:-https://evm-rpc.devnet.toonprotocol.dev}
# shellcheck disable=SC1091
source e2e/.env.devnet-actors

MARKET_MOCK=$(node -p 'require("./deployments/devnet.json").addresses.marketMock')
MARKET_REAL=$(node -p 'require("./deployments/devnet.json").addresses.marketReal')
USDC=$(node -p 'require("./deployments/devnet.json").addresses.usdc')
IMAGE_ID=0x$(node -p 'require("./predicates/ARTIFACTS.json").image_id')
# bytes32 form of the predicate's Arweave tx id (base64url -> 32 raw bytes)
ARWEAVE_TX=0x$(node -p 'Buffer.from(require("./predicates/ARTIFACTS.json").arweave_tx,"base64url").toString("hex")')

OUT=e2e/out
mkdir -p "$OUT"
RESULTS=$OUT/results.env
: > "$RESULTS"
FAILED=0

record() { echo "$1=$2" >> "$RESULTS"; echo "[rec] $1=$2"; }

# tx <privkey> <to> <sig> [args...] -> "txhash block gasUsed" (fails on revert)
tx() {
  local key=$1; shift
  local out
  out=$(cast send --rpc-url "$RPC" --private-key "$key" --json "$@")
  node -e '
    const j = JSON.parse(process.argv[1]);
    if (j.status !== "0x1") { console.error("TX REVERTED", j.transactionHash); process.exit(1); }
    console.log(j.transactionHash, parseInt(j.blockNumber, 16), parseInt(j.gasUsed, 16));
  ' "$out"
}

# tx_retry — like tx, but retries for a couple of minutes. Used for the first
# tx after a window boundary, where a marginal clock skew between this host
# and the devnet anvil could make cast's gas estimation revert.
tx_retry() {
  local i
  for i in $(seq 1 12); do
    if tx "$@"; then return 0; fi
    echo "[retry $i/12] window may not be open yet; sleeping 10s" >&2
    sleep 10
  done
  return 1
}

bal() { cast call "$USDC" 'balanceOf(address)(uint256)' "$1" --rpc-url "$RPC" | awk '{print $1}'; }
withdrawable() { cast call "$1" 'getWithdrawable(uint256,address)(uint256)' "$2" "$3" --rpc-url "$RPC" | awk '{print $1}'; }
resolution() { # 11th field of the Market struct: 0=Unresolved 1=ResolvedYes 2=ResolvedNo
  cast call "$1" 'getMarket(uint256)((address,bytes32,bytes32,bytes32,uint256,uint256,uint256,uint256,uint256,uint256,uint8,address,uint256))' "$2" --rpc-url "$RPC" \
    | sed 's/[()]//g' | cut -d',' -f11 | tr -d ' '
}

assert_eq() { # assert_eq <name> <expected> <actual>
  if [ "$2" = "$3" ]; then
    echo "ASSERT OK   $1 = $2"; record "ASSERT_$1" "OK expected=$2 actual=$3"
  else
    echo "ASSERT FAIL $1: expected $2 got $3"; record "ASSERT_$1" "FAIL expected=$2 actual=$3"; FAILED=1
  fi
}

wait_past() { # wait_past <unix_ts> <label> — wait until local wall clock is safely past ts
  local t=$1 label=$2 n
  while :; do
    n=$(date +%s)
    [ "$n" -gt $((t + 15)) ] && break
    echo "[wait] $label: $((t + 15 - n))s remaining"
    sleep 10
  done
}

# --------------------------------------------------------------------------
# Phase 1 — proofs (idempotent; inputs don't depend on marketId, so proofs
# are generated up front and windows stay short)
# --------------------------------------------------------------------------
PROVER=predicates/target/x86_64-unknown-linux-musl/release/e2e-prover
ELF=$OUT/matmul-guest.canonical.bin
if [ ! -x "$PROVER" ]; then
  echo "[build] e2e-prover"
  (cd predicates && cargo build --release -p e2e-prover --target x86_64-unknown-linux-musl)
fi
# CANONICAL ELF (matches ARTIFACTS.json / the on-chain image id) — never a local rebuild.
[ -s "$ELF" ] || gunzip -kc predicates/artifacts/matmul-guest.canonical.bin.gz > "$ELF"
[ -s "$OUT/proof-dev.json" ] || "$PROVER" --elf "$ELF" --mode dev --rank-bound 49 > "$OUT/proof-dev.json"
if [ ! -s "$OUT/proof-groth16.json" ]; then
  echo "[prove] real Groth16 (local STARK + docker SNARK wrap; this takes minutes)"
  "$PROVER" --elf "$ELF" --mode groth16 --rank-bound 49 > "$OUT/proof-groth16.json"
fi

jproof() { node -p "require('./$OUT/proof-$1.json').$2"; }
[ "$(jproof dev image_id)" = "$IMAGE_ID" ] || { echo "dev proof image_id mismatch vs ARTIFACTS.json"; exit 1; }
[ "$(jproof groth16 image_id)" = "$IMAGE_ID" ] || { echo "groth16 proof image_id mismatch vs ARTIFACTS.json"; exit 1; }
SOL_HASH=$(jproof dev solution_hash)
PARAMS_HASH=$(jproof dev market_params_hash)
JOURNAL=$(jproof dev journal_hex)          # identical journal bytes in both proofs
SEAL_DEV=$(jproof dev seal_hex)
SEAL_GROTH16=$(jproof groth16 seal_hex)
[ "$(jproof groth16 journal_hex)" = "$JOURNAL" ] || { echo "journal mismatch dev vs groth16"; exit 1; }
record GROTH16_PROVING_SECONDS "$(jproof groth16 proving_seconds)"

# --------------------------------------------------------------------------
# Phase 2 — approvals + market creation + staking
# Stakes: creator seeds NO 50, yesStaker YES 100, noStaker NO 200 (USDC, 6dp)
# --------------------------------------------------------------------------
SEED=50000000; YES_STAKE=100000000; NO_STAKE=200000000
POOL=$((SEED + YES_STAKE + NO_STAKE)) # 350 USDC

echo "== approvals =="
for key in "$CREATOR_KEY" "$YESSTAKER_KEY" "$NOSTAKER_KEY"; do
  for m in "$MARKET_MOCK" "$MARKET_REAL"; do
    tx "$key" "$USDC" 'approve(address,uint256)' "$m" 1000000000 > /dev/null
  done
done

# create_market <label> <marketAddr> <lockDelta> <deadlineDelta> <graceSecs> <bountyBps>
# sets: <label>_ID, <label>_LOCK, <label>_DEADLINE, <label>_GRACE
create_market() {
  local label=$1 maddr=$2 now lock deadline id r
  now=$(date +%s); lock=$((now + $3)); deadline=$((now + $4))
  id=$(cast call "$maddr" 'marketCount()(uint256)' --rpc-url "$RPC" | awk '{print $1}')
  r=$(tx "$CREATOR_KEY" "$maddr" \
      'createMarket(bytes32,bytes32,bytes32,uint256,uint256,uint256,uint256,uint256)' \
      "$IMAGE_ID" "$ARWEAVE_TX" "$PARAMS_HASH" "$deadline" "$5" "$lock" "$6" "$SEED")
  eval "${label}_ID=$id ${label}_LOCK=$lock ${label}_DEADLINE=$deadline ${label}_GRACE=$5"
  record "${label}_MARKET" "$maddr"
  record "${label}_ID" "$id"
  record "${label}_WINDOWS" "lock=$lock deadline=$deadline grace=$5 bountyBps=$6"
  record "${label}_CREATE_TX" "$r"
  r=$(tx "$YESSTAKER_KEY" "$maddr" 'stake(uint256,uint8,uint256)' "$id" 0 "$YES_STAKE")
  record "${label}_STAKE_YES_TX" "$r"
  r=$(tx "$NOSTAKER_KEY" "$maddr" 'stake(uint256,uint8,uint256)' "$id" 1 "$NO_STAKE")
  record "${label}_STAKE_NO_TX" "$r"
}

echo "== market A (mock, YES via dev proof) =="
create_market A "$MARKET_MOCK" 180 360 240 50
echo "== market B (real, YES via Groth16 proof) =="
create_market B "$MARKET_REAL" 180 360 600 50
echo "== market C (mock, timeout NO) =="
create_market C "$MARKET_MOCK" 120 180 60 50

# --------------------------------------------------------------------------
# Phase 3+4 — commit + reveal (miner) on A then B, withdraw, assert
# commitment = keccak256(abi.encodePacked(solutionHash, arweaveTx, miner, salt))
# --------------------------------------------------------------------------
reveal_market() { # reveal_market <label> <marketAddr> <sealHex>
  local label=$1 maddr=$2 seal=$3 id lock salt chash r pre post
  eval "id=\$${label}_ID lock=\$${label}_LOCK"
  wait_past "$lock" "$label commit window opens"
  salt=0x$(openssl rand -hex 32)
  chash=$(cast keccak "$(cast concat-hex "$SOL_HASH" "$ARWEAVE_TX" "$MINER_ADDR" "$salt")")
  r=$(tx_retry "$MINER_KEY" "$maddr" 'commit(uint256,bytes32)' "$id" "$chash")
  record "${label}_COMMIT_TX" "$r"
  r=$(tx "$MINER_KEY" "$maddr" 'reveal(uint256,bytes32,bytes32,bytes32,bytes,bytes)' \
      "$id" "$SOL_HASH" "$ARWEAVE_TX" "$salt" "$seal" "$JOURNAL")
  record "${label}_REVEAL_TX" "$r"
  assert_eq "${label}_RESOLUTION_YES" 1 "$(resolution "$maddr" "$id")"

  # parimutuel (ResolvedYes ⇒ bountyPaid = 0): yesStaker is the entire YES
  # pool, so payout = stake * winnersShare / yesPool = 100 * 350/100 = 350 USDC.
  assert_eq "${label}_WITHDRAWABLE_YES" "$POOL" \
    "$(withdrawable "$maddr" "$id" "$YESSTAKER_ADDR")"
  assert_eq "${label}_WITHDRAWABLE_CREATOR" 0 "$(withdrawable "$maddr" "$id" "$CREATOR_ADDR")"
  assert_eq "${label}_WITHDRAWABLE_NO" 0 "$(withdrawable "$maddr" "$id" "$NOSTAKER_ADDR")"
  pre=$(bal "$YESSTAKER_ADDR")
  r=$(tx "$YESSTAKER_KEY" "$maddr" 'withdraw(uint256)' "$id")
  record "${label}_WITHDRAW_YES_TX" "$r"
  post=$(bal "$YESSTAKER_ADDR")
  assert_eq "${label}_YES_USDC_DELTA" "$POOL" $((post - pre))
}

reveal_market A "$MARKET_MOCK" "$SEAL_DEV"
reveal_market B "$MARKET_REAL" "$SEAL_GROTH16"

# --------------------------------------------------------------------------
# Phase 5 — market C timeout: keeper settles after deadline+grace, NO side wins
# --------------------------------------------------------------------------
wait_past $((C_DEADLINE + C_GRACE)) "C reveal window closes"
BOUNTY=$((POOL * 50 / 10000))                    # 1.75 USDC
WINNERS_SHARE=$((POOL - BOUNTY))
NO_POOL=$((SEED + NO_STAKE))

pre=$(bal "$KEEPER_ADDR")
r=$(tx_retry "$KEEPER_KEY" "$MARKET_MOCK" 'settleTimeout(uint256)' "$C_ID")
record C_SETTLE_TX "$r"
post=$(bal "$KEEPER_ADDR")
assert_eq C_RESOLUTION_NO 2 "$(resolution "$MARKET_MOCK" "$C_ID")"
assert_eq C_KEEPER_BOUNTY "$BOUNTY" $((post - pre))
assert_eq C_WITHDRAWABLE_YES 0 "$(withdrawable "$MARKET_MOCK" "$C_ID" "$YESSTAKER_ADDR")"

pre=$(bal "$NOSTAKER_ADDR")
r=$(tx "$NOSTAKER_KEY" "$MARKET_MOCK" 'withdraw(uint256)' "$C_ID")
record C_WITHDRAW_NO_TX "$r"
post=$(bal "$NOSTAKER_ADDR")
assert_eq C_NO_USDC_DELTA $((NO_STAKE * WINNERS_SHARE / NO_POOL)) $((post - pre))

pre=$(bal "$CREATOR_ADDR")
r=$(tx "$CREATOR_KEY" "$MARKET_MOCK" 'withdraw(uint256)' "$C_ID")
record C_WITHDRAW_CREATOR_TX "$r"
post=$(bal "$CREATOR_ADDR")
assert_eq C_CREATOR_USDC_DELTA $((SEED * WINNERS_SHARE / NO_POOL)) $((post - pre))

echo
if [ "$FAILED" -ne 0 ]; then
  echo "LIFECYCLE RUN: ASSERTION FAILURES — see $RESULTS"
  exit 1
fi
echo "LIFECYCLE RUN: ALL ASSERTIONS PASSED — see $RESULTS"
