# Predicate authoring template

Copy this crate to author a new capability-market predicate ([toon-meta#119](https://github.com/toon-protocol/toon-meta/issues/119) story 1). A predicate is a frozen, deterministic Rust check compiled to a RISC Zero guest image; its 32-byte **image ID** is what a market pins on-chain, and its **journal** (see `crates/journal`) is what `CapabilityMarket.sol` decodes at reveal.

## Layout

```
crates/<your-predicate>/
├── Cargo.toml              # lib crate: your logic + host tests
├── src/lib.rs              # check(market_params, submission) -> bool  ← ALL logic here
├── methods/
│   ├── Cargo.toml          # risc0-build glue ([package.metadata.risc0] methods = ["guest"])
│   ├── build.rs            # risc0_build::embed_methods()
│   ├── src/lib.rs          # exposes <NAME>_GUEST_ELF + <NAME>_GUEST_ID
│   └── guest/
│       ├── Cargo.toml      # own [workspace]; riscv32 target
│       └── src/main.rs     # env::read ×3 → lib::evaluate → env::commit_slice
└── tests/                  # host tests; optionally a dev-mode proving test
```

The split is deliberate: **everything reviewable lives in `src/lib.rs`** and runs under plain `cargo test` on the host; the guest `main.rs` is a fixed ten-line I/O shell you copy verbatim (only the library crate name changes). The flagship `crates/matmul` is a fully worked example including a dev-mode proving test (`tests/e2e_prove.rs`).

## Authoring steps

1. `cp -r crates/template crates/<name>` and rename the packages in the three `Cargo.toml`s (`template` → `<name>`, `template-methods` → `<name>-methods`, `template-guest` → `<name>-guest`) and the crate name in `methods/guest/src/main.rs`.
2. Implement `check(market_params: &[u8], submission: &[u8]) -> bool` in `src/lib.rs`. Rules:
   - **Total, never panics.** Malformed bytes ⇒ `false`. A panicking guest can't produce the `verdict: false` proof a challenger may need.
   - **Document your byte encodings** for `market_params` and `submission` in the crate docs (the journal commits `sha256` of the raw bytes, so the encoding is part of the proposition).
   - Keep `evaluate()` as-is — it assembles the canonical journal.
3. Write host tests: positive vectors, and negative vectors for every subtle bug an adversarial reviewer might try ([toon-meta#122](https://github.com/toon-protocol/toon-meta/issues/122) review process).
4. `cargo test -p <name>` until green.

## Build pipeline: guest binary + image ID

### Dev loop (this workspace)

Building `<name>-methods` compiles the guest with the rzup-managed RISC-V toolchain and embeds the ELF and image ID as Rust constants:

```bash
rzup install                      # once: cargo-risczero, r0vm, guest rust toolchain
cargo build --release -p <name>-methods
```

Extract the image ID (it's `[u32; 8]`; the canonical byte form is `risc0_zkvm::sha::Digest::from(<NAME>_GUEST_ID)`):

```rust
println!("{}", risc0_zkvm::sha::Digest::from(matmul_methods::MATMUL_GUEST_ID));
```

Dev-mode proving (executes the real guest, skips the STARK — journal bytes are real):

```bash
RISC0_DEV_MODE=1 cargo test -p <name>
```

### Canonical (reproducible) build — REQUIRED before minting

Local builds are **not** guaranteed bit-reproducible across machines/toolchains. The image ID committed on-chain MUST come from RISC Zero's dockerized reproducible build:

```bash
cargo risczero build --manifest-path crates/<name>/methods/guest/Cargo.toml
```

This builds in a pinned Docker image and prints the image ID; the ELF lands under `target/riscv-guest/.../docker/`. Reproducibility requirement ([toon-meta#119](https://github.com/toon-protocol/toon-meta/issues/119) story 1 acceptance): run the docker build twice (ideally on two machines / in CI) and require identical image IDs before the ID is pinned in a `createMarket` transaction. Version-pin `risc0-*` crate versions and the rzup toolchain in the repo — a zkVM version bump changes image IDs and is an explicit protocol migration.

### Arweave upload (documented, not executed here)

Predicate bytes must be retrievable forever so anyone can re-derive the image ID and audit the market ([toon-meta#121](https://github.com/toon-protocol/toon-meta/issues/121) eligibility rule 2). Upload the **canonical docker-built guest ELF** through the Turbo pipeline from [toon-meta#112](https://github.com/toon-protocol/toon-meta/issues/112):

1. Take `target/riscv-guest/<name>-methods/<name>-guest/docker/<name>-guest.bin` (the canonical ELF).
2. Upload via the #112 Turbo pipeline with its funded JWK (#112 story 6 is the precondition); record the returned `arweave_tx_id`.
3. Verify retrievability before minting: fetch the bytes back from an Arweave gateway and recompute the image ID with `risc0_binfmt::compute_image_id(&elf)` — it must equal the pinned image ID.
   - Spec note for #121: the eligibility text says `sha256(bytes) == imageId`, but a RISC Zero image ID is a structured digest of the loaded memory image, **not** a plain sha256 of the ELF file. The client-side check must recompute the image ID as above. (Flagged for the envelope-spec doc.)
4. Record `(image_id, arweave_tx_id)` in the market's creation parameters.

### Production proving

The same guest proves for real by dropping `RISC0_DEV_MODE` (local `r0vm` CPU proving works but is slow; GPU provers / Kalypso are #119 story 5, out of scope for this crate).

## Journal contract (do not change)

The guest commits exactly `journal::Journal::encode()` — the 97-byte canonical `journal-v1` layout — via `env::commit_slice`. Never `env::commit(&journal)`: that would route through RISC Zero's word-oriented serde and break `sha256(journal)` on-chain. See `crates/journal` for the byte layout, endianness notes, and the golden vectors the Solidity decoder is tested against.
