# Review packet — sat (small 3-SAT floor)

**Status: logic-ready, NOT mint-ready.** Ships as a host-testable verifier
library plus the full manifest layer — **no compiled RISC Zero guest and no
frozen image ID yet**. You can review and sign off the *logic* today; the
predicate cannot back a live market until a `methods/guest` shell is compiled
(following `predicates/crates/template`) and its image ID frozen the way
matmul's was. Do not record a *mint-ready* sign-off. Read `docs/review/README.md`
first (process + shared manifest/journal binding).

Source under review (read-only; do not edit):
- `predicates/crates/sat/src/lib.rs` — the money-critical logic
- `predicates/crates/sat/tests/adversarial.rs` — agent-contributed negative vectors
- `predicates/crates/sat/README.md`

---

## (a) Exact proposition statement

> **"A satisfying assignment for pinned 3-SAT instance I is published by
> date D."**

`I` is a small hand-picked 3-SAT instance, content-addressed as a pinned market
parameter. The claimed assignment is the submission. Verdict `true` iff the
instance and the assignment are both well-formed **and every clause of I is
satisfied** under the assignment. This is a mechanism proof (calibratable
difficulty), not a frontier problem — its job is to exercise the mint/reveal
round-trip. "by date D" is a contract-side window (guest verdict is
time-independent; README §3).

---

## (b) English ↔ Rust mapping

`src/lib.rs`.

| Proposition clause | Enforced by | Notes |
|---|---|---|
| "3-SAT instance I" (DIMACS signed literals, 1-indexed) | `Instance {num_vars: u32, clauses: Vec<Vec<i32>>}` (L41-45); `validate_instance` requires each clause has **exactly 3** literals (L98-101) | `+k` ⇒ var k true, `-k` ⇒ var k false, `0` invalid |
| "pinned" (content-addressed) | `encode_instance`/`decode_instance` — `num_vars u32 LE ‖ num_clauses u32 LE ‖ 3×i32 LE per clause` (L153-219); embedded as the manifest `instance` VALUE (L242-249) | one canonical byte string per instance |
| "satisfying assignment" | `check` (L117-140): each clause satisfied iff *some* literal is true — `assignment[var-1]` for `+k`, `!assignment[var-1]` for `-k` (L125-139); verdict `true` iff **every** clause satisfied | `verdict` (L145-147) folds malformed → false |
| well-formed instance within ceilings | `validate_instance` (L91-111): `1 ≤ num_vars ≤ MAX_VARS(1024)`; `≤ MAX_CLAUSES(4096)`; every literal in `1..=num_vars` (`checked_abs` handles `i32::MIN`) | ceilings pin proving cost |
| assignment matches the instance | `check` length gate: `assignment.len() == num_vars` else `AssignmentLengthMismatch` (L119-124) | truncated *or* padded assignment rejected |
| "published" (bytes committed) | `evaluate` → `submission_hash = sha256(submission)`, `market_params_hash = sha256(manifest_bytes)` (L273-276) | binds exact submitted assignment bytes |

Submission decode (`decode_assignment`, L195-204): one byte per variable,
**strictly** `0x00`/`0x01`; any other byte is malformed. This makes the
submission→assignment map injective, so `submission_hash` binds the assignment
uniquely (no two byte strings decode to the same assignment).

---

## (c) Input manifest consumed + submission format

**Manifest** (`encode_manifest(instance, frozen_clock)`, L242-249), canonical
`manifest-v1`:

| Entry name | Kind | Bytes |
|---|---|---|
| `frozen_clock` | VALUE | `u64` LE deadline literal — audit only |
| `instance` | VALUE | canonical `encode_instance` bytes of the pinned 3-SAT instance I |
| `submission` | SLOT | zero-length; the late-bound assignment |

`marketParamsHash = sha256(manifest_bytes)`. Guest reads `(image_id,
manifest_bytes, submission)`, extracts `instance` by name, decodes it, decodes
the submission as the assignment, runs `verdict`. **No image ID frozen yet** —
this manifest shape is defined but no guest hashes it.

**Submission** (canonical assignment): `num_vars` bytes, each `0x00` (false) /
`0x01` (true), in variable order 1..=num_vars.

**Pinned launch fixture** (`fixture`, L291-312): 4 variables, 5 clauses
`[1,2,-3], [-1,3,4], [-2,-3,4], [1,-2,-4], [-1,2,3]`; a known satisfying
assignment is `x1=T, x2=F, x3=T, x4=T`.

---

## (d) Known edge cases & subtleties (from agent review — start here)

Cited from PR #1 and its adversarial-review comment (agent `ALLiDoizCode`,
commit `6da02b7`), and `tests/adversarial.rs`.

1. **Clause evaluation correctness (signed DIMACS, 1-indexed)** — *resolved.*
   Cross-validated against an independently-written reference evaluator on 10k
   randomized instances/assignments — zero drift
   (`adversarial.rs::fuzz_check_matches_reference_evaluator`). **Reviewer B:
   confirm the sign convention `+k ⇔ var k true` reading L129-133.**
2. **Vacuous truth — a zero-clause instance is trivially satisfied** — *accepted
   (the one real semantic finding).* A well-formed instance with **zero clauses**
   returns `Ok(true)` for any correctly-sized assignment (the `for clause` loop is
   empty). This is *not* a submitter exploit — the instance is pinned and reviewed
   at market creation — but it was undocumented while the README claimed the
   English matches the code "precisely". Now stated in `lib.rs` (L14-16) + README
   and pinned by `adversarial.rs::zero_clause_instance_is_vacuously_satisfied`.
   **Reviewer obligation: when reviewing a *pinned instance* for an actual market,
   reject any instance with zero clauses.** (Note: the empty *clause* `[]` is a
   different thing — it is *malformed*, `ClauseNotThreeLiterals`, L98-101.)
3. **Malformed-input matrix holds, no panics** — *resolved.* 20k random-byte
   decoder fuzz: no panics; everything `decode_instance` accepts re-validates as
   well-formed 3-SAT (`fuzz_decoders_never_panic`, `fuzz_mutated_valid_encoding`).
   Strict length (`bytes.len() != expected_len`) rejects truncation at every cut
   point *and* trailing garbage. `i32::MIN` literal handled via `checked_abs`
   (L104). Assignment bytes strictly `0x00/0x01`.
4. **Instance/assignment binding** — *resolved.* Length mismatch (truncated or
   padded) → malformed-false; ceilings checked *before* any per-clause work;
   `Vec::with_capacity` capped at `MAX_CLAUSES` so a hostile header can't force a
   large allocation (L173-176).
5. **Journal placeholder → shared crate** — *resolved.* The local `Journal`
   placeholder was field-for-field identical to the canonical crate; deduped to
   the shared `journal` crate in PR #5 (commit `d255307`). No semantic drift.

---

## (e) Spec-exploit worksheet (Reviewer B — try to break it)

Goal: an assignment that **PASSES the code but violates the intent** (judged
satisfying when it is not), or **FAILS the code but satisfies the intent**.

- **Literal sign / index convention.** The single most likely place for a
  falsely-PASS/FAIL. Confirm `+k` reads `assignment[k-1]` true and `-k` reads it
  false — an off-by-one on the 1-indexing, or an inverted sign, would silently
  mis-judge. Construct an assignment that satisfies I under the *correct*
  convention but not under a flipped one, and confirm the code agrees with the
  correct convention. Check the `-1` in `assignment[var - 1]` (L128) against a
  literal referencing variable 1 and variable `num_vars`.
- **Vacuous truth.** Confirm subtlety #2: a zero-clause instance passes for any
  assignment. As a reviewer of a *real* pinned instance, your job is to reject
  such an instance at market creation. Also probe: can a pinned instance be
  crafted so it is *trivially* satisfiable (e.g. a tautological clause set)
  disguised as hard? That is a market-design review, not a code bug — flag it.
- **Malformed-instance handling.** Since the instance is pinned (not attacker-
  supplied), the risk is a *market creator* pinning a malformed instance that the
  guest silently judges. Confirm each malformation is a clean verdict-false, never
  a panic and never an accidental `true`: empty clause, 2- or 4-literal clause,
  literal `0`, out-of-range literal, `i32::MIN`, `num_vars=0`, ceiling breaches,
  undecodable blob.
- **Submission aliasing.** Try two different submission byte strings that might
  decode to the same assignment — the strict `0x00/0x01` rule should make this
  impossible (any other byte is malformed-false), so `submission_hash` binds the
  assignment uniquely. Confirm a `0x02` byte is rejected, not coerced to `true`.
- **Length games.** Assignment of length `num_vars ± 1`; empty assignment against
  a nonzero instance. Confirm `AssignmentLengthMismatch`.
- **Manifest tampering.** A manifest whose `instance` VALUE differs → different
  `market_params_hash` → contract reveal fails
  (`check_manifest_extracts_instance_by_name`,
  `evaluate_binds_manifest_hash_not_raw_params`). Confirm you cannot swap in an
  easier instance without changing `marketParamsHash`. Missing `instance` entry →
  verdict false.

---

## (f) Negative vectors the reviewer must confirm are rejected

Each must be verdict `false` (or clean `Malformed`), never a panic:

1. An assignment that falsifies at least one clause (e.g. `x1=T,x2=T,x3=T,x4=F`
   falsifies clause `[-2,-3,4]`) → `Ok(false)`.
2. Assignment length ≠ `num_vars` (5 or 3 values against 4 vars) → `AssignmentLengthMismatch`.
3. An instance with an empty clause `[]` → `ClauseNotThreeLiterals`.
4. A clause with a literal for a nonexistent variable (var 5 in a 4-var instance) → `LiteralOutOfRange`.
5. A clause with a `0` literal, or `i32::MIN` → malformed, verdict false, no panic.
6. `num_vars = 0` → `VarCountOutOfRange`.
7. `num_vars > 1024` or `clauses > 4096` → ceiling rejection.
8. Truncated / trailing-garbage instance blob → `BadEncoding`; assignment byte `0x02` → `BadEncoding`.
9. Manifest that does not parse or lacks `instance` → verdict false.

---

## (g) Sign-off

> Two independent reviewers, independent of the author and of each other. See
> `README.md` §1. **This predicate is logic-ready only** — a sign-off here clears
> the *logic*; it does NOT authorize a live market until a guest is compiled and
> its image ID frozen (open dependency, record it in your notes).

### Reviewer 1

- Name: ______________________________
- Date (ISO): __________________________
- Verdict (`APPROVE` / `APPROVE-WITH-NOTES` / `REJECT`): __________________________
- Scope acknowledged (logic-only; guest/image-ID pending): ☐
- Notes:



### Reviewer 2

- Name: ______________________________
- Date (ISO): __________________________
- Verdict (`APPROVE` / `APPROVE-WITH-NOTES` / `REJECT`): __________________________
- Scope acknowledged (logic-only; guest/image-ID pending): ☐
- Notes:
