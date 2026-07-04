# Review packet — matmul (flagship)

**Status: mint-ready.** Real compiled RISC Zero guest; frozen canonical image ID
`80db88cd4190c8adf12b58c2aca51812b7a3ca82fa04a0a61c8f91b9dc9985b2`
(`predicates/ARTIFACTS.json`). This is the only launch predicate that can back a
real market once two human sign-offs land. Read `docs/review/README.md` first
(process + shared manifest/journal binding).

Source under review (read-only for you; do not edit):
- `predicates/crates/matmul/src/lib.rs` — the money-critical logic
- `predicates/crates/matmul/methods/guest/src/main.rs` — the 10-line guest shell
- `predicates/crates/matmul/tests/adversarial.rs` — agent-contributed negative vectors
- `predicates/ARTIFACTS.json` — frozen image ID + Arweave tx + canonical manifest

---

## (a) Exact proposition statement

> **"A rank-≤46 bilinear scheme for 4×4 matrix multiplication over GF(2),
> symbolically verified, is published by 2027-07-01."**

Given a claimed bilinear scheme `{(uᵢ, vᵢ, wᵢ) for i in 1..=r}`, the verifier
symbolically expands `Σᵢ wᵢ (uᵢ·A)(vᵢ·B)` as polynomials in the 32 entries of
`A` and `B` **over GF(2)** and checks equality with the 16 entries of `AB`, and
checks `r ≤ bound` where `bound` comes from the market's pinned params (46 for
the flagship). This is a genuine open problem — best known is rank 47 (2022
AlphaTensor); lower bounds are mid-30s — so a rank-≤46 witness would be new
mathematics.

The "by 2027-07-01" clause is enforced by the **contract's** commit/reveal
windows, not the guest (the guest verdict is time-independent; see README §3).

---

## (b) English ↔ Rust mapping

Every clause of the proposition tied to the code that enforces it. `src/lib.rs`.

| Proposition clause | Enforced by | Notes |
|---|---|---|
| "bilinear scheme `{(uᵢ,vᵢ,wᵢ)}`" | `Triple {u,v,w: u16}` (L76-84); `decode_scheme` parses 6 bytes/triple, `u16` BE each (L170-182) | bit `j` of each mask ↔ matrix entry `(j/4, j%4)`, row-major |
| "4×4 … over GF(2)" | `ENTRIES = 16` (L73); all masks `u16` (16 bits); `check_identity` arithmetic is XOR (`^=`) and AND (bit tests) only — i.e. GF(2) (L145-167) | no field other than GF(2) is representable; coefficients are single bits |
| "symbolically verified … `Σᵢ wᵢ(uᵢ·A)(vᵢ·B)` equals `AB`" | `check_identity` (L145-167): for each output `o=(p,q)`, accumulate the 16×16 GF(2) coefficient matrix `m[j] ^= vᵢ` for every `j∈uᵢ` of every triple with `wᵢ ∋ o`; compare to `target` = the matmul tensor `(AB)[p][q]=Σ_t A[p][t]B[t][q]` (coefficient 1 at `(4p+t, 4t+q)`) | this is the polynomial-identity check, not a spot-check on sample matrices — soundness rests here |
| "rank-≤46" (bound is a market parameter) | `verify_scheme` rank check `triples.len() > bound ⇒ RankExceeded` (L118-123); `FLAGSHIP_RANK_BOUND = 46` (L70); bound decoded from `market_params` (L196-204) | `bound` is the 32-byte BE uint from the manifest's `market_params` VALUE |
| well-formedness / anti-gaming (implicit in "a scheme") | `EmptyScheme` (L110-112); `DegenerateProduct` for `u=0 ∨ v=0 ∨ w=0` (L113-117); `DuplicateProduct` for equal `(u,v)` (L126-132) | canonical-form checks; see subtlety #2 below for their precise (limited) strength |
| "published" (bytes committed on-chain) | `evaluate` → `journal::predicate_journal` sets `submission_hash = sha256(submission)`, `market_params_hash = sha256(manifest_bytes)` (L279-282) | binds the exact submitted bytes and the frozen manifest |

**Check order** (`verify_scheme`, L109-134): empty → degenerate → rank →
duplicate → polynomial identity. Any failure ⇒ `verdict false`. The core `check`
(L218-226) and manifest-driven `check_manifest` (L265-273) both fold every
malformation to `false`, never panic.

---

## (c) Input manifest consumed + submission format

**Manifest** (`encode_manifest(rank_bound, frozen_clock)`, L252-259), canonical
`manifest-v1`, three VALUE/SLOT entries (stored in ascending name order):

| Entry name | Kind | Bytes |
|---|---|---|
| `frozen_clock` | VALUE | `u64` LE deadline literal — **audit only**, guest never branches on it |
| `market_params` | VALUE | 32-byte BE uint rank bound (`abi.encode(uint256(bound))`), top 28 bytes must be zero. Flagship = 46 |
| `submission` | SLOT | zero-length; the late-bound scheme |

`marketParamsHash = sha256(manifest_bytes)`. Flagship canonical manifest bytes
and hash are frozen in `ARTIFACTS.json` (`manifest_sha256 =
0a029dce…ee0e`, `frozen_clock = 1735689600`, `rank_bound = 46`). The guest reads
`(image_id, manifest_bytes, submission)`, extracts `market_params` by name, and
runs the unchanged `check`.

**Submission** (`matmul-submission-v1`): `r` triples concatenated, 6 bytes each,
`u16` BE `u ‖ v ‖ w`; `len = 6r`, `r ≥ 1`. Each `u16` is a GF(2) coefficient
vector over 16 entries, bit `j` ↔ entry `(j/4, j%4)` row-major, big-endian.

---

## (d) Known edge cases & subtleties (from agent review — start here)

Cited from PR #3 and PR #3's adversarial-review comment (agent
`ALLiDoizCode`), and `tests/adversarial.rs`.

1. **GF(2) symbolic expansion completeness** — *resolved.* Exhaustive
   single-coefficient error injection: for every output entry (16) × every
   monomial position `(j,k)` (256), corrupting exactly that coefficient of a
   valid scheme is rejected — 4096 cases
   (`adversarial.rs::every_output_and_every_coefficient_position_is_checked`).
   Cross-validated symbolic ≡ brute-force on all 256 basis pairs under mutation
   fuzz (`fuzz_symbolic_check_equals_basis_evaluation`). **Reviewer B: re-run or
   spot-check this — it is the soundness core.**

2. **Rank cannot be under-counted** — *resolved / accepted.* Every cancellation
   trick (exact duplicates, zero triples, near-duplicate splits) *inflates* `r`,
   never deflates it. Any accepted scheme is `r ≤ bound` nonzero rank-1 terms
   summing to the matmul tensor, which by definition witnesses rank ≤ bound.
   **Soundness does not rest on the duplicate check at all.**

3. **The near-duplicate rank subtlety (the one real defect found in PR #3)** —
   *resolved (doc corrected).* The module doc originally claimed the duplicate
   check ensures "`r` = number of distinct products actually used". False: a
   split-product near-duplicate `(u,v,w) → (u, v⊕d, w) + (u, d, w)` evades the
   exact-`(u,v)` scan and still passes the identity — proven by
   `adversarial.rs::split_product_padding_counts_toward_rank`. Harmless for
   soundness (both halves count toward `r`, so it can only push a scheme *over*
   the bound), but the stated security claim was stronger than the code. Doc now
   (L57-65) states the checks enforce *canonical form only*, with the soundness
   argument spelled out. **Accepted; confirm the corrected doc matches the code.**

4. **Degenerate zero triples** — *resolved.* `u=0 ∨ v=0 ∨ w=0` rejected before
   the rank check (`DegenerateProduct`), tested
   (`tests::degenerate_zero_vectors_rejected`).

5. **Duplicate `(u,v)` products** — *resolved.* Two triples with equal `(u,v)`
   can cancel/merge over GF(2); rejected as non-canonical
   (`tests::duplicate_product_rejected_even_when_identity_holds`, r=51 passes
   identity but is caught).

6. **Malformed encodings fold to false, never panic** — *resolved.* 20k-iter raw
   bytes fuzz (`fuzz_check_raw_bytes_never_panics`); trailing garbage after the
   32-byte params rejected (`market_params_trailing_garbage_rejected`); the guest
   commits `verdict=false` for a truncated submission in dev-mode e2e (a real
   provable FALSE journal, not a trap).

7. **`market_params_hash` binding vs manifest** — *resolved by the manifest
   layer* (PR #10 / commit `44afd26`). The PR #3 review flagged that the guest
   originally hashed `sha256(raw params)` while the envelope spec wanted
   `sha256(manifest)`. Now the guest hashes the **manifest bytes**
   (`evaluate` → `market_params_hash = sha256(manifest_bytes)`,
   `tests::evaluate_builds_canonical_journal_from_manifest` asserts it is *not*
   `sha256(raw params)`). **Confirm the on-chain `createMarket` commits the same
   definition, or no reveal will ever match** — this is the contract's obligation
   (out of scope for the predicate, in scope to note).

8. **No submission size cap** — *accepted.* A multi-MB submission is decoded
   before the rank check. Linear cost and only the prover pays, so it is a
   prover-side cycle concern, not a soundness issue. (Note the O(r²) duplicate
   scan runs only after `r ≤ bound` caps `r`, so the quadratic path is bounded.)

---

## (e) Spec-exploit worksheet (Reviewer B — try to break it)

Goal: a submission that **PASSES the code but violates the intent** (a rank-≤46
scheme that is not actually a correct GF(2) matmul scheme, or that a
mathematician would not call rank ≤ 46), **or** one that **FAILS the code but
satisfies the intent** (a genuine rank-≤46 scheme the code wrongly rejects).

- **Near-duplicate / cancelling products inflating vs deflating rank.** Take a
  valid scheme, split one product `(u,v,w)` into `(u, v⊕d, w) + (u, d, w)`. Does
  it pass identity? (Yes — subtlety #3.) Confirm both halves count toward `r`,
  so it can only push *over* 46, never manufacture a sub-46 witness. Now try the
  reverse: can any pair of listed triples cancel so that the *effective* rank is
  below the *listed* `r`? Argue why `r = triples.len()` is the honest count even
  under cancellation (subtlety #2). **This is the highest-value attack — spend
  time here.**
- **GF(2) vs GF(other) confusion.** The intent is GF(2). Confirm the code cannot
  be read as GF(2^k) or ℤ: coefficients are single bits, arithmetic is XOR/AND.
  Try to smuggle a scheme that is valid over a *different* field but is accepted
  here (it should either fail identity or be meaningless). Confirm the `w` fan-out
  (`(t.w >> o) & 1`) treats each output bit independently — no carry, no field
  extension.
- **Degenerate zero terms.** Confirm `u=0`, `v=0`, `w=0` are each rejected
  *individually* (all three in the `||`), and that a triple with, say, `w=0` can't
  slip a "free" term to pad rank downward. Try `u`/`v`/`w` = all-ones or single
  high bit (bit 15, entry (3,3)).
- **Manifest `rank_bound` tampering.** The bound lives in the manifest VALUE and
  is bound into `market_params_hash`. Try: (i) a 32-byte params with a nonzero
  high byte (should be `MalformedMarketParams`, L200-202); (ii) `bound` beyond
  `u32` (rejected by the top-28-zero rule); (iii) a manifest whose
  `market_params` says 49 but presented against the flagship market — confirm the
  *hash* differs so the contract's reveal fails (`tests::wrong_manifest_yields_
  wrong_params_hash`), i.e. you cannot swap in a laxer bound without changing
  `marketParamsHash`. (iv) A manifest missing `market_params` entirely → verdict
  false (`malformed_manifest_is_false_not_panic`).
- **Identity spot-check escape.** Confirm the check is *symbolic* (all monomials),
  not a sample of concrete `A,B` — otherwise a scheme correct on the sampled pairs
  but wrong in general would pass. Read `check_identity` and confirm `target` is
  the full tensor for every output.
- **Bound boundary.** Exactly `r = 46` accepts; `r = 47` rejects. Confirm the
  comparison is `>` not `>=` (L118) and that `r ≤ bound` (not `<`).

---

## (f) Negative vectors the reviewer must confirm are rejected

Each must yield `verdict false` (or a clean `Reject`), never a panic:

1. Valid rank-49 Strassen⊗Strassen scheme against flagship bound 46 → `RankExceeded{49,46}`.
2. Any single flipped `u`-coefficient or `w`-coefficient of a valid scheme → `NotMatmul`.
3. Same triple appended twice (cancels over GF(2), identity still holds, r=51≤64) → `DuplicateProduct`.
4. A triple with `u=0`, or `v=0`, or `w=0` → `DegenerateProduct`.
5. Empty submission (`r=0`) → `EmptyScheme`; also `[]` bytes and non-multiple-of-6 length → `MalformedSubmission`.
6. `market_params` of wrong length (31 bytes) or with a dirty high byte → `MalformedMarketParams`.
7. 33-byte / 64-byte params (trailing garbage after the uint) → rejected.
8. Manifest that does not parse, or is missing `market_params` → verdict false.
9. Split-product near-duplicate that *does* pass identity but pushes r past the bound → `RankExceeded` (must not be accepted as sub-46).

---

## (g) Sign-off

> Two independent reviewers, independent of the author and of each other. See
> `README.md` §1 for what a sign-off asserts. For this **mint-ready** predicate,
> also confirm the frozen image ID `80db88cd…985b2` in `ARTIFACTS.json` is the
> one that will be pinned on-chain.

### Reviewer 1

- Name: ______________________________
- Date (ISO): __________________________
- Verdict (`APPROVE` / `APPROVE-WITH-NOTES` / `REJECT`): __________________________
- Image ID confirmed pinned (`80db88cd…985b2`): ☐
- Notes:



### Reviewer 2

- Name: ______________________________
- Date (ISO): __________________________
- Verdict (`APPROVE` / `APPROVE-WITH-NOTES` / `REJECT`): __________________________
- Image ID confirmed pinned (`80db88cd…985b2`): ☐
- Notes:
