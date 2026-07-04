# Devnet e2e — full market lifecycle (recorded run)

Recorded run of [`e2e/run-lifecycle.sh`](run-lifecycle.sh) against the live TOON
devnet on **2026-07-03**, driving three markets end-to-end with real USDC value
movements and — the headline — **a real Groth16 proof verified on-chain by
`RiscZeroGroth16Verifier` inside `reveal()`**. All 15 assertions passed;
exact parimutuel deltas matched to the unit (6-decimal USDC).

- RPC: `https://evm-rpc.devnet.toonprotocol.dev` (shared anvil, chainId 31337 —
  no time cheatcodes used; all windows were short real-time windows)
- Deployment: [`deployments/devnet.json`](../deployments/devnet.json)
  (`marketMock` = `0xd1aAc47737FdF1bb124121CE8eF0bee47dEd9AeA`,
  `marketReal` = `0x46879970393eB1a4E55CE868b77BD59DEfAEF459`)
- Predicate: canonical matmul guest, image id
  `0x660d47e33136b07e362d5efac8669ee3d31603df5aa5c12dba41f91156e8ecff`
  (exactly [`predicates/ARTIFACTS.json`](../predicates/ARTIFACTS.json); the
  prover ran the committed canonical ELF, not a local rebuild)
- Market params: rank bound **49** (`abi.encode(uint256(49))`,
  `market_params_hash = sha256 = 0x218cd422…4652`); submission = the known-valid
  rank-49 Strassen⊗Strassen scheme from `matmul::schemes::strassen_4x4_rank49()`
  (`solution_hash = sha256 = 0x89c4eed6…1ece`). Bound 46 is the unsolved
  flagship and was deliberately not used.
- Actors: five fresh `cast wallet new` throwaway wallets (creator, yesStaker,
  noStaker, miner, keeper), each faucet-funded 100 ETH + 10,000 USDC. Keys are
  gitignored (`e2e/.env.devnet-actors`).
- Stake shape (all markets): creator seeds **50 NO**, yesStaker **100 YES**,
  noStaker **200 NO** → pool 350 USDC. Bounty 50 bps.

## Market A — `marketMock`, resolved YES via dev-mode (fake-seal) proof

Market id **1** on `marketMock`. Windows: lock `1783121422`, deadline
`1783121602`, grace 240 s. Dev-mode receipt over the canonical ELF, encoded by
`risc0_ethereum_contracts::encode_seal` → 36-byte `0xffffffff` selector seal,
accepted by `RiscZeroMockVerifier` only.

| Step | Tx | Block | Gas |
|---|---|---|---|
| createMarket (+seed 50 NO) | `0x9803edddf944a008ac3bd1afc26f229d1b73007863dab0cfe225311d7e4793af` | 1506 | 322,829 |
| stake YES 100 | `0xce34f8a09fa705fcba2ee676e5f2d447cfaf2a43921244da5279a83163498d1d` | 1507 | 100,394 |
| stake NO 200 | `0xbfdc33424b2d929e3af11b8dbed81c1f834872f605da79a7b953e360dd0ce647` | 1508 | 103,196 |
| commit (miner) | `0xb24e94a60192cc6f97786bfe2ac2f02dd6ca215e8eb440a7213acf1dfab3274f` | 1515 | 75,303 |
| reveal (dev seal) | `0xa7cd74a7fe51edbae2a25c217d2f6b8a9cd19c56e11b910a50de544d26b8b605` | 1516 | 85,167 |
| withdraw (yesStaker) | `0x527dfe1f0ef565891ad46a6f41332b4924722630c0a202a27939600b6242ee0e` | 1517 | 81,149 |

Assertions (6-decimal USDC units):

| Check | Expected | Actual | |
|---|---|---|---|
| resolution | 1 (ResolvedYes) | 1 | OK |
| yesStaker withdrawable (`100 × 350/100`, bountyPaid = 0 on reveal path) | 350,000,000 | 350,000,000 | OK |
| creator withdrawable (seeded NO — loses) | 0 | 0 | OK |
| noStaker withdrawable | 0 | 0 | OK |
| yesStaker balance delta on withdraw | +350,000,000 | +350,000,000 | OK |

## Market B — `marketReal`, resolved YES via REAL Groth16 proof (headline)

Market id **0** on `marketReal`. Windows: lock `1783121424`, deadline
`1783121604`, grace 600 s. The proof was generated **before** market creation
(journal binds only image id / params hash / submission hash — not marketId),
so the windows stayed short.

Proving (all local, `predicates/crates/e2e-prover`, mode `groth16`):
`RISC0_PROVER=ipc` → installed `r0vm` 3.0.5 STARK proving + succinct
compression, then STARK→SNARK wrap in the
`risczero/risc0-groth16-prover:v2025-04-03.1` docker image.
**Wall clock: 91.3 s** (1:31.49 by `/usr/bin/time`, WSL2 x86-64 box; the
one-time docker image pull is excluded). Seal selector `0x73c457ba` matched
`deployments/devnet.json`, and the seal was pre-flighted with an `eth_call` to
`RiscZeroGroth16Verifier.verify(seal, imageId, sha256(journal))` — accepted —
before the live reveal.

| Step | Tx | Block | Gas |
|---|---|---|---|
| createMarket (+seed 50 NO) | `0xd237ce481c3ec11d49731739e58678a07a53dd25b5dec634e9cea6601146e392` | 1509 | 339,941 |
| stake YES 100 | `0x97795f86c296cdafde81b6eda22072a40f7fcddfb877a4ae0c850945a23655e0` | 1510 | 100,382 |
| stake NO 200 | `0x7b6df84938891ea0681afd99e7c890fb4f71dfd30fd408b9b51ca7f67e1ba7b8` | 1511 | 103,184 |
| commit (miner) | `0x4ea7bbb7d7300b9af63bf8cd1e71952ce7d79d884c64ff926f6da304a2872cfc` | 1518 | 75,291 |
| **reveal (real Groth16 seal, verified on-chain)** | `0x904c5ba61bf2480a015ca7b9bb09af96f5efdd0744a233aa8c4843379c8d8dab` | 1519 | **315,272** |
| withdraw (yesStaker) | `0x44a451b3dc2b41619944866dcc917ed4d8d62f88fbfef8668f3c009f755fdf88` | 1520 | 76,337 |

Reveal gas 315,272 vs 85,167 on the mock path ⇒ the on-chain Groth16
verification costs ≈ 230k gas.

Assertions — identical shape to market A, all OK: resolution 1 (ResolvedYes);
yesStaker withdrawable / balance delta 350,000,000 exactly; creator and
noStaker withdrawable 0.

## Market C — `marketMock`, timeout → resolved NO with keeper bounty

Market id **2** on `marketMock`. Windows: lock `1783121367`, deadline
`1783121427`, grace 60 s. Nobody committed; after `deadline + grace` the keeper
(5th wallet, no stake) called `settleTimeout`.

| Step | Tx | Block | Gas |
|---|---|---|---|
| createMarket (+seed 50 NO) | `0x042fbb9930471081d03b6556bba5518c89625357dff85c06ee13cddbb4604f53` | 1512 | 305,729 |
| stake YES 100 | `0x9c2cb1cbba8c89c8a5f3cc5b308f1e3fe3fb8336d96200ed703648296f36583c` | 1513 | 100,394 |
| stake NO 200 | `0xac633451007cb054587d83ecd7ce772f1a4ca002a6638f2ac2180c86a3b2740e` | 1514 | 103,196 |
| settleTimeout (keeper) | `0xcf4a82ccfa9f1a92ba611a151b4d68fa8308c6c02ae80c99c3031ede8daef824` | 1521 | 100,060 |
| withdraw (noStaker) | `0xd00d779af7a755b2176ad37bc699d54c9c7385e5f0fc74df82f98ffd6660da6b` | 1522 | 81,170 |
| withdraw (creator) | `0x1e6b884a1c4536013f40be0d0532bf439011c6e8ac9490c9566a45293f17204d` | 1523 | 76,370 |

Parimutuel math: bounty = `350 × 50/10000` = **1.75 USDC**; winnersShare =
348.25; NO pool = 250.

| Check | Expected | Actual | |
|---|---|---|---|
| resolution | 2 (ResolvedNo) | 2 | OK |
| keeper bounty balance delta | +1,750,000 | +1,750,000 | OK |
| yesStaker withdrawable | 0 | 0 | OK |
| noStaker delta (`200 × 348.25/250`) | +278,600,000 | +278,600,000 | OK |
| creator delta (`50 × 348.25/250`) | +69,650,000 | +69,650,000 | OK |

Payouts + bounty sum to the 350 USDC pool exactly (no rounding dust here).

## Deviations / notes

- `e2e-prover` runs proving through the external `r0vm` server
  (`RISC0_PROVER=ipc`) rather than the in-process `local` prover: the
  in-process prover needs the heavy `prove` feature of `risc0-zkvm`, while
  `ipc` uses the already-installed `r0vm` 3.0.5 binary. Same machine, same
  proof, same `encode_seal` output.
- Dev-mode and Groth16 runs committed byte-identical 97-byte journals
  (asserted by the runner), confirming journal determinism across seal kinds.
- The reveal-path `arweaveTx` argument reuses the predicate's Arweave tx id
  (bytes32 of base64url-decoded `62uFTWV3AJFYdz1C4JFf2wye98r4VC3t-bhpdHWEbfw`)
  as a stand-in for a solution upload; the contract only binds it into the
  commitment hash.
- Total lifecycle wall clock (markets + real-time window waits, proving
  cached): ≈ 6.5 min. Nothing in the run touched anvil time.

## Reproducing

```sh
cp e2e/.env.devnet-actors.example e2e/.env.devnet-actors  # fill in 5 funded wallets
./e2e/run-lifecycle.sh
```

The runner is idempotent about artifacts (builds `e2e-prover`, gunzips the
canonical ELF, generates/caches both proofs under `e2e/out/`, gitignored) and
writes a structured record to `e2e/out/results.env`.

---

## Re-run: input-manifest binding (capability-market#4, 2026-07-04)

Regenerated the canonical matmul image after the guests moved to the input
manifest (`marketParamsHash = sha256(canonical manifest bytes)`, NOT
`sha256(raw params)`), then re-ran the **mock-verifier** lifecycle on live
devnet to exercise the new binding on-chain. The mock path still runs the real
guest and the contract still enforces every journal field check on `reveal`
(`j.marketParamsHash == m.marketParamsHash`), so it fully proves the new
preimage end-to-end. The real-Groth16 market was **not** re-run: the verifier
is unaffected by the params-hash preimage (that path was already proven above).

### Regenerated canonical artifact

| field | old | new |
|---|---|---|
| image_id | `660d47e3…56e8ecff` | `80db88cd4190c8adf12b58c2aca51812b7a3ca82fa04a0a61c8f91b9dc9985b2` |
| elf_sha256 | `f931359c…f094557d` | `ea087289e12f06e43889942608af12baba95a2457769f035c1a9707685ac5e2f` |
| elf_bytes | 175880 | 178032 |
| elf_gzip_bytes | 88048 | 89268 |
| arweave_tx (ELF gz) | `62uFTWV3…HWEbfw` | `KRYHACfle56dsRYuEMXodlvpMSvpz2FCv1ApzxnWmzU` |
| manifest_arweave_tx | — | `J2Ie4J5K6cYifr1oaKWckFA6bnVE356UEKGkCW5TS8g` |

- Docker `cargo risczero build` run **twice from clean** → byte-identical ELF
  (sha256 `ea087289…`) and identical image ID `80db88cd…` both times.
- `check-predicate` round-trip (fetch → gunzip → `compute_image_id`) on the
  refreshed `predicates/artifacts/matmul-guest.canonical.bin.gz` matches the
  new image ID; the manifest bytes fetched back from Arweave hash to
  `0a029dce…` (the flagship rank-46 `marketParamsHash`).

### Mock market re-run (marketMock `0xd1aAc47737FdF1bb124121CE8eF0bee47dEd9AeA`)

Fresh wallets, funded from the devnet faucet (ETH + test USDC). Short REAL-TIME
windows only — no `evm_increaseTime`/`anvil_*` on the shared anvil.

| assertion | result |
|---|---|
| prover: `market_params_hash == sha256(manifest_bytes)` | OK (`0x619ab7e8…d5b7d710`, rank-49 manifest) |
| on-chain `market.marketParamsHash` at createMarket `== sha256(manifest)` | **OK** |
| `reveal(…)` with the manifest-bound journal | resolved **ResolvedYes** (1) |

- Market id `3` on marketMock; created, YES/NO staked, committed after the
  real-time lock window, revealed with the dev-mode seal + manifest-bound
  97-byte journal. The contract accepted the reveal, proving the on-chain
  `marketParamsHash = sha256(manifest)` field check passes end-to-end.
- Latest devnet block at run time: `0x60b`.

The runner `run-lifecycle.sh` was updated to feed the manifest (via
`e2e-prover`, which now emits `manifest_hex`/`manifest_sha256`) and to assert
both the prover binding and the on-chain `marketParamsHash == sha256(manifest)`
in `create_market`.
