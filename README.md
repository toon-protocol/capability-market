# capability-market

TOON capability market — a permissionless parimutuel market on the capability frontier. Frozen, machine-checkable predicates are minted as propositions; a RISC Zero zkVM proof of the sealed predicate settles escrow trustlessly on Base.

Umbrella epic: [toon-meta#84](https://github.com/toon-protocol/toon-meta/issues/84). Envelope spec: `docs/predicate-envelope.md` in toon-meta ([toon-meta#121](https://github.com/toon-protocol/toon-meta/issues/121)).

## Layout

- `contracts/` — Foundry project: `CapabilityMarket.sol` escrow on Base ([toon-meta#120](https://github.com/toon-protocol/toon-meta/issues/120))
- `predicates/` — Rust cargo workspace: RISC Zero guest programs + authoring toolchain ([toon-meta#119](https://github.com/toon-protocol/toon-meta/issues/119), [toon-meta#122](https://github.com/toon-protocol/toon-meta/issues/122))
  - `crates/journal` — shared journal struct `{image_id, market_params_hash, submission_hash, verdict}`
  - `crates/template` — predicate authoring template (guest shell)
  - `crates/matmul` — flagship: 4×4 matmul rank-≤46 over GF(2)
  - `crates/sat` — small 3-SAT floor predicate
  - `crates/difftest` — differential-testing divergence predicate

Decided forks (settlement on Base, direct-USDC escrow, commit-reveal, USDC-only, RISC Zero not TEE) are recorded in toon-meta#84 — do not relitigate here.
