# Kalypso optional prover path — GPU-less proof outsourcing

> **Story:** toon-meta#119 story 5 · **Decision:** toon-meta#84 _"Prover market:
> Kalypso, optional"_ — the fork is decided; Kalypso is the prover market, and it
> is a **convenience, never a dependency**.

A miner in the capability market must submit a RISC Zero proof that its
submission satisfies a market's predicate (for the flagship market: "this is a
valid rank-≤N bilinear scheme for 4×4 GF(2) matmul"). Proving is the expensive
step. A miner **with** hardware proves locally (r0vm on CPU, or a GPU). A miner
**without** hardware can outsource proof generation to
[Kalypso](https://kalypso.marlin.org/), Marlin's decentralized ZK proving
marketplace, paying in **USDC** — and gets back a proof in the identical form the
on-chain reveal expects.

This directory implements that optional path behind a clean provider interface,
with the local prover as the always-available fallback.

## Miner UX

```
GPU-less miner                Kalypso marketplace              CapabilityMarket.sol
──────────────                ───────────────────              ────────────────────
 pick witness  ──createAsk──▶  market (imageID ⇄ USDC)
 (rank-49)        (pay USDC)     │  matches a STAKED prover
                                 │  under a slashing-backed SLA
                                 ▼
                              risc0 prover generates seal
 reconstruct   ◀──getProof───   ProofCreated(seal)
 journal (det.)                                        ──reveal(seal, journal)──▶ verify + payout
```

The miner spends only USDC. It never runs r0vm, never provisions a GPU, and
never holds POND (the prover's staking/slashing collateral).

## What this ships

| file | role |
|------|------|
| `prove.mjs` | CLI: `--source local\|kalypso\|auto` → `{seal_hex, journal_hex}` ready for `reveal(...)` |
| `lib/providers.mjs` | `LocalProver` (delegates to r0vm `e2e-prover`) + `KalypsoProver` (kalypso-sdk) behind one interface |
| `lib/journal.mjs` | self-contained port of the canonical 97-byte `journal-v1` encoding (authority: `predicates/crates/journal`) |
| `lib/matmul.mjs` | rank-49 Strassen⊗Strassen witness + market-params/submission encodings (authority: `predicates/crates/matmul`) |
| `test/self-check.mjs` | headless proof (no r0vm, no network) that the JS ports match the Rust crates byte-for-byte |
| `.env.kalypso.example` | Kalypso deployment config template (USDC-only for the miner) |

Same output shape from either source — the reveal path is prover-agnostic:

```json
{ "source": "local|kalypso",
  "seal_hex": "0x…",       // risc0-ethereum encode_seal output (proof arg)
  "journal_hex": "0x…" }   // canonical 97-byte journal-v1 (journal arg)
```

## Quick start

```sh
# 0. one-time: gunzip the canonical guest ELF the image ID commits to
gzip -dc ../../predicates/artifacts/matmul-guest.canonical.bin.gz > matmul-guest.elf

# headless self-check — validates the JS journal/matmul ports vs the Rust crates
npm test

# LOCAL prover (default), dev mode — real 97-byte journal, mock-verifier seal (marketMock)
node prove.mjs --source local --mode dev

# LOCAL prover, real Groth16 seal (marketReal) — minutes; pulls the risc0 groth16 docker image
node prove.mjs --source local --mode groth16

# KALYPSO prover — outsource proving, pay USDC (needs a live Kalypso deployment; see below)
cp .env.kalypso.example .env.kalypso   # fill in, then: set -a; source .env.kalypso; set +a
npm install                            # pulls ethers + kalypso-sdk
node prove.mjs --source kalypso --max-price-usdc 500000 --max-time 1800

# AUTO — use Kalypso if configured+reachable, else fall back to the local prover
node prove.mjs --source auto --mode groth16
```

## Market setup (one-time, per image ID)

Kalypso pairs **every (circuit, payment-token) into its own market**. To let
miners outsource proofs for our matmul predicate:

1. **Register the market** — `KalypsoSdk.MarketPlace().createPublicMarket(marketMetaData, verifier, slashingPenalty, ivsPcrs)`.
   - `marketMetaData` pins our RISC Zero **image ID** `0x80db88cd4190c8adf12b58c2aca51812b7a3ca82fa04a0a61c8f91b9dc9985b2`
     (`predicates/ARTIFACTS.json`).
   - `verifier` is a risc0 Groth16 verifier the prover network honors, producing
     a seal our `RiscZeroGroth16Verifier` (`deployments/devnet.json`,
     selector `0x73c457ba`) also accepts.
   - `slashingPenalty` is the prover's bond — the teeth behind the SLA.
   - Payment token = **USDC** (`config.payment_token`); creating a market costs
     `MARKET_CREATION_COST` in that token.
2. **A prover joins** — a staked generator registers for the market with our guest
   ELF and comes online. (Permissionless, but a prover must actually run *our*
   guest; the network doesn't auto-support arbitrary circuits.)

## USDC payment flow

- Miner approves USDC to the ProofMarketplace, then `createAsk({ marketId,
  proverData: submission, reward: maxPriceUsdc, expiry, timeTakenForProofGeneration:
  maxTime, refundAddress })`.
- The `reward` (USDC) is escrowed and released to the prover on valid-proof
  delivery; on SLA breach it is **refunded** to `refundAddress` and the prover's
  bond is **slashed**.
- POND/restaking secures the prover side (see Marlin × Symbiotic restaking); the
  miner's exposure is USDC only.

## SLA / slashing model (as found)

Kalypso is an orderbook marketplace: requesters post asks stating whether they
prioritize **price** or **time-to-proof**; staked provers (hardware operators)
match and fulfil them. The matching engine runs inside a Marlin **Oyster TEE**
node. Delivery is bound by a deadline (`timeTakenForProofGeneration` / ask
expiry); a prover that misses it or returns an invalid proof is **slashed**
against its staked collateral, and the requester is refunded — decentralized
prover networks are being secured via **restaked ETH through Symbiotic**. This is
the SLA that makes outsourcing safe: the miner is protected by the prover's bond,
not by trusting the prover.

## Journal reconstruction (why outsourcing is safe & cheap)

The canonical journal is **fully determined** by `(image_id, manifest_bytes,
submission, verdict)` — see `lib/journal.mjs`. Per toon-meta#121 /
capability-market#4 the journal's `market_params_hash` is
**`sha256(canonical manifest-v1 bytes)`**, NOT `sha256(raw params)`: the rank
bound rides as the `market_params` VALUE entry of a `manifest-v1` TLV
(authority: `predicates/crates/manifest`), alongside the `frozen_clock` literal
and the late-bound `submission` SLOT. So a Kalypso prover only needs to
return the **seal**; we reconstruct the exact 97 journal bytes locally and the
seal binds to `sha256(journal)`. `test/self-check.mjs` proves our JS
reconstruction is **byte-identical** to what the guest commits — including a
byte-for-byte cross-check of the `manifest-v1` encoder against the Rust
`manifest` crate for rank bounds 46 and 49. A prover cannot lie
about the journal: `CapabilityMarket.reveal` re-derives `sha256(journal)`,
enforces `journal.imageId == market.imageId`, and checks the verdict.

## Honest current state — what works headless here vs. what needs Kalypso

**Verified on this box (headless, no Kalypso account):**

- ✅ `npm test` — JS `journal`/`matmul` ports match the Rust crates byte-for-byte.
- ✅ `--source local --mode dev` — real 97-byte journal + mock seal, end-to-end via r0vm 3.0.5.
- ✅ Journal reconstructed by the **Kalypso path** (`predicateJournal` → 97 bytes)
  equals the guest's committed journal exactly.
- ✅ `--source auto` correctly falls back to the local prover when Kalypso is
  unconfigured; `--source kalypso` refuses to run (no fabricated proof).
- ✅ Kalypso **reachability preflight** connects to a live chain: pointed at
  Arbitrum Sepolia it returned `{chainId: 421614, proofMarketplaceDeployed: false}`
  — chain transport works; it truthfully reports no marketplace at a placeholder
  address. `npm`/registry and Arbitrum RPCs are reachable from here.

**Not completable headless (needs a real Kalypso testnet setup):**

- ❌ A full Kalypso round-trip. Blockers, all outside this box:
  1. **Deployment config isn't public** — `KalspsoConfig` needs live ProofMarketplace
     / EntityRegistry / GeneratorRegistry addresses **and** the Oyster **matching-engine
     enclave URL**. The `kalypso-sdk` repo ships no testnet address book; these come
     from a live Marlin deployment.
  2. **No market exists for our image ID** — someone must `createMarket` pairing
     `0x80db88cd…` with USDC and pay `MARKET_CREATION_COST`.
  3. **No staked prover runs our guest** — the permissionless network doesn't
     auto-support an arbitrary custom RISC Zero guest; a generator must register
     with our ELF, stake, and come online. Without one, an ask is never matched.
  4. **Funded testnet wallet** — USDC (reward + market cost) on the Kalypso chain.

The integration is therefore complete and exercised up to the network boundary;
the seal-returning transport (`createAsk` → `getProofByAskId`) is implemented
against `kalypso-sdk@1.0.57`'s real API but **not executed against a live market**,
because no reachable Kalypso market/prover for our circuit exists to execute it.
No step fakes a Kalypso proof.

## SDK reference (date-sensitive — captured 2026-07-03)

- **Package:** [`kalypso-sdk`](https://github.com/marlinprotocol/Kalypso-SDK) `1.0.57` (npm), tested with `ethers@6.6.6`.
- **Key API:** `KalypsoSdk(signer, config)` → `.MarketPlace()` → `createPublicMarket(...)`,
  `approvePaymentTokenToMarketPlace(amount)`, `createAsk(marketId, proverData, reward,
  assignmentDeadline, blocksForProofGeneration, refundAddress, secretType, secretBuffer)`,
  `getProofByAskId(askId, fromBlock)`, `getAskState(askId)`, `submitProof(...)`.
- **Config (`KalspsoConfig`):** `payment_token`, `staking_token`, `generator_registry`,
  `attestation_verifier`, `entity_registry`, `proof_market_place`, `tee_verifier_deployer`,
  enclave endpoints (`matchingEngineEnclave.url`, …), `checkInputUrl`, `attestationVerifierEndPoint`.
- **Architecture:** orderbook marketplace; matching engine in a Marlin **Oyster TEE**;
  RISC Zero supported among multiple zkVMs; prover networks secured by **restaking via Symbiotic**.
- **Sources:**
  <https://github.com/marlinprotocol/Kalypso-SDK> ·
  <https://docs.marlin.org/learn/kalypso/> ·
  <https://www.marlin.org/kalypso> ·
  <https://blog.marlin.org/securing-decentralized-zk-prover-networks-through-restaking>

> Kalypso's deployed addresses, endpoints, and SDK surface change over time. The
> above is a snapshot; re-verify `KalspsoConfig` fields and live addresses against
> the current Marlin docs before a real run.
