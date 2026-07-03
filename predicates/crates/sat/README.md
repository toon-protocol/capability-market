# `sat` — small 3-SAT floor predicate

Launch predicate 2 of [toon-meta#122](https://github.com/toon-protocol/toon-meta/issues/122). A mechanism proof, not a frontier problem: a tiny verifier with calibratable difficulty for testing the mint/reveal round-trip.

## Proposition statement (matched precisely to the code)

> **"A satisfying assignment for pinned 3-SAT instance I is published by date D."**
>
> Precisely: the verdict is `true` if and only if all of the following hold —
>
> 1. The pinned instance I is well-formed 3-SAT: `1 ≤ num_vars ≤ 1024`, at most 4096 clauses, every clause has **exactly three** literals, and every literal is a non-zero signed integer whose absolute value is in `1..=num_vars`. (An empty clause is rejected as malformed — it is *not* treated as trivially-unsatisfiable pure-SAT semantics.)
> 2. The submitted assignment decodes as exactly one boolean (byte `0x00` or `0x01`) per variable — exactly `num_vars` values, no more, no fewer.
> 3. Every clause of I contains at least one literal made true by the assignment (literal `+k` is true iff variable `k` is assigned true; `-k` iff assigned false).
>
> Any malformed input — truncated or padded assignment, non-boolean byte, wrong clause arity, zero or out-of-range literal, size ceilings exceeded, blob that does not decode — yields verdict `false` (a clean rejection). The check never panics: a panicking guest cannot produce a PASS proof, but the failure mode is spec'd here so code and English agree exactly.

Implementation: `src/lib.rs` (`check` / `verdict`, `decode_instance`, `decode_assignment`). Pinned launch instance: `fixture::pinned_instance()` — 4 variables, 5 clauses.

## Input manifest sketch

```
input_manifest:
  - name: "instance"            # canonical encoding of the pinned 3-SAT instance I
    hash: sha256:<hash of canonical instance bytes>
    encoding: raw_bytes         # num_vars u32 LE | num_clauses u32 LE | clauses as 3 × i32 LE each
  - name: "submission"          # the claimed assignment
    hash: <bound at reveal time>
    encoding: raw_bytes         # one byte per variable: 0x00 = false, 0x01 = true
  - name: "frozen_clock"        # deadline D as pinned data
    value: <unix timestamp>
```

Size ceilings (`MAX_VARS = 1024`, `MAX_CLAUSES = 4096`) are pinned in the predicate source, so proving cost is bounded by construction.

## Guest wiring

This crate is a pure-Rust, host-testable library (`cargo test -p sat`). The RISC Zero guest `main.rs` wiring — `env::read()` of manifest inputs, `verdict()` call, `env::commit_slice(&journal.encode())` of the 97-byte journal-v1 encoding (never serde `env::commit()` — the contract digests the raw bytes) — follows the `template` crate once `feat/risc0-toolchain-matmul` merges. The `Journal` struct in `src/journal.rs` is a local copy of the toon-meta#121 envelope, to be replaced by the shared `journal` crate (see `TODO(dedup)` marker).

## Follow-up before any market opens

- **Adversarial review** (two independent reviewers per toon-meta#122) — **not yet performed**
- Image-ID freeze after the RISC Zero build (content-hash commitment)
- Arweave upload of the predicate bytes, `arweave_tx_id` recorded
- Bug-bounty window funded and opened pre-launch
