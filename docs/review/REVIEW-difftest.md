# Review packet — difftest (differential-testing divergence)

**Status: logic-ready, NOT mint-ready.** Ships as a host-testable verifier
library plus the full manifest layer — **no compiled RISC Zero guest and no
frozen image ID yet**. Review and sign off the *logic* today; the predicate
cannot back a live market until a `methods/guest` shell is compiled (following
`predicates/crates/template`) and its image ID frozen the way matmul's was. Do
not record a *mint-ready* sign-off. Read `docs/review/README.md` first (process +
shared manifest/journal binding).

Source under review (read-only; do not edit):
- `predicates/crates/difftest/src/lib.rs` — the money-critical logic (incl. the pinned stack-machine interpreter)
- `predicates/crates/difftest/tests/adversarial.rs` — agent-contributed negative vectors
- `predicates/crates/difftest/README.md`

---

## (a) Exact proposition statement

> **"An input on which pinned programs P1 and P2 produce different outputs is
> published by date D."**

Arbitrary native programs can't run in a zkVM guest, so **P1 and P2 are pinned
bytecode blobs for a tiny deterministic stack machine defined in this crate**
(12 instructions, wrapping `u64`). The claimed divergence-witness input is the
submission. Verdict `true` iff **both** programs `HALT` cleanly within the step
budget on the claimed input **and** their outputs differ. This is a genuine
open-frontier pattern when P1/P2 are two versions of a real system; it also
proves the "predicate embeds pinned programs as evidence" pattern. "by date D" is
a contract-side window (guest verdict is time-independent; README §3).

---

## (b) English ↔ Rust mapping

`src/lib.rs`.

| Proposition clause | Enforced by | Notes |
|---|---|---|
| "pinned programs P1, P2" | `MANIFEST_PROGRAM_1/2` VALUE entries (L275-277); pinned bytecode for the machine in `mod op` (L42-67) | 12 opcodes: PUSH/POP/ADD/SUB/MUL/DIV/DUP/SWAP/INPUT/JMP/JZ/HALT |
| "produce different outputs" | `check` (L248-253): `(Ok(out1), Ok(out2)) => out1 != out2`; any failed run ⇒ `false` | output = value popped at `HALT` (L234-236) |
| "an input on which…" | `decode_input` (L112-124): ≤ `MAX_INPUT_WORDS(64)` little-endian `u64` words, length multiple of 8; the submission | `verdict` (L257-262) folds malformed input → false |
| "deterministic machine, bounded cost" | `run` (L129-242): `MAX_STEPS(100_000)` step ceiling checked *before each* instruction (L144); `MAX_STACK(256)`; `MAX_PROGRAM_BYTES(4096)`; all arithmetic `wrapping_*` | every op deterministic; no nondeterministic source |
| "both terminate cleanly" | verdict `true` requires **both** `run` calls return `Ok` (clean `HALT`), else `false` | non-termination = `StepBudgetExceeded` → `Err` → false |
| trap semantics (no accidental output) | `ExecError` variants (L71-93): underflow, overflow, unknown opcode, truncated immediate, OOB jump, OOB input index, div-by-zero, pc-off-end, HALT-on-empty | every trap ⇒ `Err` ⇒ verdict false in **both** argument orders |
| "published" | `evaluate` → `submission_hash = sha256(submission)`, `market_params_hash = sha256(manifest_bytes)` (L327-330) | binds exact submitted input bytes |
| step budget is *visible* in market params | `MANIFEST_STEP_BUDGET` VALUE must equal compiled-in `MAX_STEPS` else verdict false (L308-321) | manifest cannot advertise a ceiling the guest does not honour |

**Machine semantics reviewers must confirm:** `HALT` pops and returns the top of
stack (traps `StackUnderflow` on empty — so "halt with empty stack" is *not* a
clean termination). `DIV` traps on zero divisor. `JMP/JZ` targets are
byte-addressed and must be in-bounds. `INPUT idx` traps if `idx ≥ input.len()`.
Running off the end without `HALT` is `PcOutOfBounds` (a trap, not a clean halt).

---

## (c) Input manifest consumed + submission format

**Manifest** (`encode_manifest(p1, p2, frozen_clock)`, L292-301), canonical
`manifest-v1`:

| Entry name | Kind | Bytes |
|---|---|---|
| `frozen_clock` | VALUE | `u64` LE deadline literal — audit only |
| `program_1` | VALUE | P1 bytecode blob |
| `program_2` | VALUE | P2 bytecode blob |
| `step_budget` | VALUE | `u64` LE — **must equal compiled-in `MAX_STEPS`** or verdict false |
| `submission` | SLOT | zero-length; the late-bound input |

`marketParamsHash = sha256(manifest_bytes)`. Guest reads `(image_id,
manifest_bytes, submission)`, extracts both programs + checks `step_budget`, runs
`verdict`. **No image ID frozen yet.**

**Submission** (canonical input): `n` little-endian `u64` words, `n ≤ 64`, length
a multiple of 8.

**Pinned launch fixture** (`fixture`, L369-385): `P1 = identity` (`f(x)=x`),
`P2 = square` (`g(x)=x·x` wrapping). They agree exactly on the fixed points of
squaring (x=0 and x=1) and diverge everywhere else — a divergence witness exists
but not every input is one.

---

## (d) Known edge cases & subtleties (from agent review — start here)

Cited from PR #1 and its adversarial-review comment (agent `ALLiDoizCode`,
commit `6da02b7`), and `tests/adversarial.rs`.

1. **Determinism meta-property (the core soundness lever)** — *resolved.* 20k
   random-bytecode fuzz asserting a program can **never** be judged divergent from
   itself (`fuzz_random_bytecode_never_panics_and_never_self_diverges`). This
   catches any nondeterministic op, uninitialized read, or state leak between the
   two runs. No opcode is nondeterministic; all arithmetic wrapping; `DIV` traps
   on zero. **Reviewer B: this is the property that makes a divergence witness
   meaningful — confirm no shared mutable state between the two `run` calls
   (each `run` allocates its own `stack`, `pc`, `steps`).**
2. **Ceilings enforced before resource use** — *resolved.* Program size checked
   before execution; input word count before materializing words; stack ceiling
   trips at exactly `MAX_STACK` (`stack_ceiling_exact`); step budget checked
   before each instruction so `JMP 0` burns exactly `MAX_STEPS` then
   `StepBudgetExceeded`.
3. **"Both must halt cleanly" is symmetric** — *resolved.* Every trap variant
   (underflow, overflow, unknown opcode, truncated immediate, OOB jump, OOB input
   index, div-zero, pc-off-end, HALT-on-empty) poisons the verdict in **both**
   argument orders (`trap_on_either_side_never_passes`).
4. **Jump into the middle of a PUSH immediate** — *resolved.* Byte-addressed
   jumps can land inside a multi-byte immediate; this is legal, deterministic, and
   panic-free — the byte is re-interpreted as an opcode
   (`jump_into_immediate_is_deterministic_not_panic`). Confirm this is *intended*
   (it is: the machine is byte-addressed, so it is well-defined behaviour, not a
   bug).
5. **Malformed input blobs** — *resolved.* Unaligned (not multiple of 8) or > 64
   words → verdict false through `verdict()`, no panic
   (`fuzz_verdict_raw_bytes_never_panics`,
   `tests::negative_malformed_input_blob`).
6. **Step-budget declaration binding** — *resolved.* A manifest advertising a
   `step_budget` the interpreter doesn't enforce is rejected (verdict false), even
   with valid programs + input (`tests::wrong_step_budget_declaration_rejected`).
   This keeps the pinned proving-cost ceiling honest and visible.
7. **Journal placeholder → shared crate** — *resolved.* Deduped to the shared
   `journal` crate in PR #5 (`d255307`); no semantic drift.

---

## (e) Spec-exploit worksheet (Reviewer B — try to break it)

Goal: an input that **PASSES the code but violates the intent** (judged a
divergence when the programs don't really diverge, or when one didn't cleanly
compute an output), or **FAILS the code but is a real divergence witness**.

- **Nondeterminism.** The whole predicate is meaningless if the machine is
  nondeterministic. Read every opcode for any source of nondeterminism
  (uninitialized stack read, order-dependence, shared state between the two runs,
  reliance on allocation addresses). Confirm each `run` is a pure function of
  `(program, input)`. Try to make P vs P (same program) diverge — it must be
  impossible (subtlety #1).
- **Step / stack budget boundaries.** Confirm the step check is `steps >=
  MAX_STEPS` *before* executing (L144), so a program that would halt on exactly
  step `MAX_STEPS` is cut as non-termination (verdict false) — is that the
  intended boundary? Confirm `MAX_STACK` trips at exactly 256 (push #257 traps),
  both for PUSH and DUP and INPUT (all three guard `stack.len() >= MAX_STACK`).
  Try a program that halts at exactly the budget edge.
- **Trap-vs-halt semantics.** The intent is "both produce an output" — a trap is
  *not* an output. Confirm: (i) `HALT` on an empty stack is `StackUnderflow`, not
  a clean halt with some default value; (ii) running off the end is `PcOutOfBounds`
  (a trap), not an implicit halt; (iii) a program that traps still can't count as
  "different output" in either order. Try to construct a case where P1 outputs a
  value and P2 *traps*, and confirm verdict is **false** (a trap is not a
  different output — it is no output).
- **Pinned-program tampering.** The programs live in the manifest VALUEs, bound
  into `market_params_hash`. Try swapping `program_1`/`program_2` bytes → different
  hash → contract reveal fails. Try a manifest missing a program, or with a
  `step_budget` ≠ `MAX_STEPS` → verdict false (subtlety #6). Confirm you cannot
  substitute a pair of trivially-diverging programs without changing
  `marketParamsHash`.
- **Divergence smuggling via wrapping.** All arithmetic wraps `u64`. Confirm that
  a genuine divergence is preserved (outputs compared as `u64`, exact inequality)
  and that wrapping can't be abused to make two *equal* mathematical results
  compare unequal or vice versa — the comparison is on the raw `u64` output, which
  is exactly what "different outputs" means for this machine.
- **Input-index and immediate edges.** `INPUT idx` with `idx` = input length
  (off-by-one) → trap. Truncated `PUSH`/`INPUT`/`JMP` immediate at end of program
  → `TruncatedImmediate`. Confirm these are traps (verdict false), not panics or
  reads past the slice.

---

## (f) Negative vectors the reviewer must confirm are rejected

Each must be verdict `false`, never a panic:

1. Identical programs (P1 == P2) on any input → false (cannot diverge from itself).
2. An input where P1 and P2 agree (x=0 or x=1 for identity vs square) → false.
3. A non-terminating program (`JMP 0`) on either side → `StepBudgetExceeded` → false.
4. Any trap on either side — div-by-zero, stack underflow, unknown opcode `0xff`,
   OOB jump, OOB input index, pc-off-end, stack overflow — → false, both orders.
5. Truncated `PUSH`/`INPUT`/`JMP` immediate → `TruncatedImmediate` → false.
6. `HALT` with an empty stack → `StackUnderflow` → false (not a clean output).
7. Empty program, or program > 4096 bytes → `ProgramSizeOutOfRange` → false.
8. Input blob not a multiple of 8, or > 64 words → malformed → false.
9. Manifest missing a program, or `step_budget` ≠ `MAX_STEPS` → false.

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
