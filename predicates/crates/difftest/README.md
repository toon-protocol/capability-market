# `difftest` — differential-testing divergence predicate

Launch predicate 3 of [toon-meta#122](https://github.com/toon-protocol/toon-meta/issues/122). Proves the pattern "predicate embeds pinned programs as evidence."

## Proposition statement (matched precisely to the code)

> **"An input on which pinned programs P1 and P2 produce different outputs is published by date D."**
>
> Precisely: P1 and P2 are bytecode blobs for the deterministic stack machine defined in this crate (fixed 12-instruction set: `PUSH POP ADD SUB MUL DIV DUP SWAP INPUT JMP JZ HALT`; wrapping `u64` arithmetic). The verdict is `true` if and only if all of the following hold —
>
> 1. The submitted input decodes as a sequence of 8-byte little-endian `u64` words, at most 64 of them.
> 2. P1, run on that input, terminates via `HALT` within the 100,000-step budget without trapping, producing a single `u64` output.
> 3. P2, run on the **same** input, likewise terminates cleanly, producing a single `u64` output.
> 4. The two outputs are unequal.
>
> Spec'd explicitly: non-termination (step budget exceeded) or **any** trap on either program — stack underflow, stack overflow (>256), unknown opcode, truncated immediate, out-of-bounds jump, `INPUT` index beyond the input, division by zero, running off the end of the program, `HALT` on an empty stack, empty or oversized (>4096-byte) program — yields verdict `false`, as does a malformed input blob. The check never panics: a panicking guest cannot produce a PASS proof, but the failure mode is spec'd here so code and English agree exactly.

Implementation: `src/lib.rs` (`run`, `check`, `verdict`, `decode_input`). Launch fixture programs: `fixture::p1_identity()` (`f(x) = x`) and `fixture::p2_square()` (`g(x) = x·x`), which agree only on the fixed points 0 and 1. (Real markets pin two versions of a real system — e.g. two SAT solvers — compiled/transpiled to this bytecode; the fixture pair calibrates the mechanism.)

## Bounded proving cost

The step ceiling (`MAX_STEPS = 100_000` per program), stack depth (`MAX_STACK = 256`), program size (`MAX_PROGRAM_BYTES = 4096`), and input size (`MAX_INPUT_WORDS = 64`) are constants in the predicate source and therefore **part of the pinned params**: total guest work is bounded by construction (≤ 2 × MAX_STEPS interpreter steps), so proving cost is bounded regardless of what bytecode or input is pinned or submitted.

## Input manifest sketch

```
input_manifest:
  - name: "p1_bytecode"         # pinned program P1
    hash: sha256:<hash of P1 blob>
    encoding: raw_bytes         # stack-machine bytecode, see src/lib.rs op module
  - name: "p2_bytecode"         # pinned program P2
    hash: sha256:<hash of P2 blob>
    encoding: raw_bytes
  - name: "submission"          # the claimed divergence witness input
    hash: <bound at reveal time>
    encoding: raw_bytes         # 8-byte LE u64 words, ≤ 64 words
  - name: "frozen_clock"        # deadline D as pinned data
    value: <unix timestamp>
```

## Guest wiring

This crate is a pure-Rust, host-testable library (`cargo test -p difftest`). The RISC Zero guest `main.rs` wiring — `env::read()` of manifest inputs, `check()` call, `env::commit_slice(&journal.encode())` of the 97-byte journal-v1 encoding (never serde `env::commit()` — the contract digests the raw bytes) — follows the `template` crate once `feat/risc0-toolchain-matmul` merges. The `Journal` struct comes from the shared `journal` crate (`predicates/crates/journal`, the toon-meta#121 envelope), re-exported as `difftest::journal`.

## Follow-up before any market opens

- **Adversarial review** (two independent reviewers per toon-meta#122) — **not yet performed**
- Image-ID freeze after the RISC Zero build (content-hash commitment)
- Arweave upload of the predicate bytes, `arweave_tx_id` recorded
- Bug-bounty window funded and opened pre-launch
