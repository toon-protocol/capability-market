# Bug bounty — capability-market launch predicates (scope draft)

**Status: DRAFT.** Two dependencies are unresolved and flagged inline below:
the **reward pool is not yet funded** and the **private disclosure channel is not
yet set up**. This document defines *scope and structure* so those two can be
finalized; it is not yet a live program. Per
[toon-protocol/toon-meta#122](https://github.com/toon-protocol/toon-meta/issues/122),
the bounty pool must be funded before the pre-launch window opens.

The bounty exists so that breaking a launch predicate earns a reward **before**
any real market opens — a soundness bug caught here is cheap; the same bug in a
live market is a rigged market with real money at stake.

---

## In scope

The three launch predicates and the shared envelope that binds them:

- `predicates/crates/matmul` — flagship (mint-ready; frozen image ID
  `80db88cd…985b2`).
- `predicates/crates/sat` — logic-ready.
- `predicates/crates/difftest` — logic-ready.
- `predicates/crates/journal` — the shared `journal-v1` envelope.
- `predicates/crates/manifest` — the shared `manifest-v1` input-manifest layer.
- The matmul guest shell `predicates/crates/matmul/methods/guest`.

### What counts as a valid finding

1. **A falsely-PASS in any launch predicate** — a submission that the verifier
   judges `verdict = true` (satisfies the proposition) but that does **not**
   satisfy the natural-language proposition in the predicate's review packet. This
   is the highest-severity class: it lets someone win a market they should lose.
   Examples of the shape: a rank > 46 matmul scheme accepted at bound 46; a
   non-satisfying SAT assignment judged satisfying; a difftest input judged
   divergent where the programs actually agree or one traps.

2. **A falsely-FAIL in any launch predicate** — a submission that genuinely
   satisfies the proposition but the verifier judges `verdict = false` (or
   rejects/panics). This lets a legitimate winner be denied, or the market
   resolve NO against a true proposition.

3. **A manifest / journal-binding bypass** — any way to make the on-chain
   commitments not bind what they claim:
   - two distinct byte strings sharing a `marketParamsHash` preimage (manifest
     non-canonicality) or a `journal` digest collision under the strict decoder;
   - a guest that commits a `market_params_hash` ≠ `sha256(manifest_bytes)`, a
     `submission_hash` ≠ `sha256(submission)`, or a journal that is not the exact
     canonical 97 bytes;
   - swapping in laxer market parameters (rank bound, SAT instance, difftest
     programs, step budget) without changing `marketParamsHash`;
   - a verdict byte or field encoding that the Rust encoder and the
     `CapabilityMarket.sol` decoder disagree on (a divergence from the golden
     vectors in `golden_journal_vectors.json` / `golden_manifest_vectors.json`).

4. **A guest that panics or diverges from its host library** on any input such
   that the falsely-PASS/FAIL or binding-bypass effect above is achievable
   in-circuit (for the mint-ready matmul guest).

A valid finding must come with a **concrete reproducing input** (bytes or a test)
and a one-line statement of which proposition clause or binding rule it breaks.

---

## Severity tiers

| Tier | Definition | Examples |
|---|---|---|
| **Critical** | A falsely-PASS in the **mint-ready** predicate (matmul), or any journal/manifest binding bypass that lets a market resolve against the intent, exploitable end-to-end (including in-circuit). | rank>46 scheme accepted; `marketParamsHash` preimage collision; a guest committing a forged verdict. |
| **High** | A falsely-PASS or falsely-FAIL in the **logic-ready** predicates (sat, difftest), or a falsely-FAIL in matmul; a binding bug not yet reachable in a live market only because the guest isn't compiled. | non-satisfying SAT assignment judged true; genuine divergence witness rejected. |
| **Medium** | A soundness-relevant spec/code gap that is not directly exploitable for a wrong resolution (e.g. an undocumented edge that a careful market creator must avoid), or a panic/DoS reachable only by the prover (who pays their own cost). | vacuous-truth style creator footgun; a guest panic on malformed input that still can't produce a PASS. |
| **Low / informational** | Doc-vs-code mismatch with no soundness impact, hardening suggestions, additional negative vectors. | the "distinct products" over-claim already fixed in PR #3. |

Final tier is set by the review team at triage; the table is guidance. A single
report may combine findings — each is scored on its own.

---

## Out of scope

- **`CapabilityMarket.sol`** — the escrow/parimutuel/commit-reveal mechanism is
  **reviewed and bountied separately** (it is #120's deliverable; see PR #2). Bugs
  in stake accounting, payout math, MEV/window handling, reentrancy, or the
  Solidity journal decoder belong to that scope — **except** where a
  predicate-side encoding causes the Rust↔Solidity binding to diverge (that is in
  scope here, item 3).
- **The RISC Zero proving stack / zkVM soundness** (that's #119's integration and
  RISC Zero's own security surface). A break in the SNARK itself is not a predicate
  bug.
- **Gas / performance** of the contract, and **prover-side cost** (a large
  submission that only makes the prover's own proof expensive — the prover pays).
- **UX**, tooling ergonomics, and anything in the repo root outside `predicates/`.
- Findings requiring a **malicious market creator** who pins a bad instance/params
  are *market-design* review, not predicate bugs — report them, but they are
  Medium at most (the packets already direct human reviewers to reject such pins
  at market creation).

---

## Reward pool structure

> **OPEN DEPENDENCY — treasury funding not yet allocated.** Amounts below are
> **placeholders (TBD)** pending a funding source. Per #122 the pool must be
> funded before the pre-launch window opens; treasury allocation is the candidate
> source and is an explicit precondition, not yet met. Do not publish reward
> figures until funding is confirmed.

| Tier | Reward (TBD) |
|---|---|
| Critical | TBD — largest share of the pool |
| High | TBD |
| Medium | TBD |
| Low / informational | TBD — may be recognition-only |

Structure proposed (to be ratified with the funded amount):

- A fixed **total pool** sized before the window opens; per-finding awards drawn
  from it, Critical-weighted.
- First valid reporter of a given root-cause is eligible; duplicates of an
  already-reported root cause are acknowledged but not separately rewarded.
- Findings that merely re-surface an edge already documented as *accepted* in a
  review packet (e.g. matmul near-duplicate rank padding, SAT zero-clause vacuous
  truth) are **not** eligible — they are known and dispositioned.

---

## Pre-launch bounty window

- The window opens **after** two human reviewers have signed off a predicate's
  packet **and** the reward pool is funded, and runs for a fixed period
  (recommended: **at least 2 weeks** of public exposure) **before** the first real
  (value-bearing) market opens on that predicate.
- For the **logic-ready** predicates (sat, difftest), the window should follow
  guest compilation + image-ID freeze — the mint-ready artifact is what a live
  market exposes, so it must be the artifact under bounty.
- No real market may open on a predicate until its window closes with no
  unresolved Critical/High finding.

---

## How to submit

> **OPEN DEPENDENCY — private disclosure channel NEEDS SETUP.** A confidential
> intake must exist before the window opens; the address below is a placeholder.

- **Private disclosure only.** A soundness bug in a predicate that is about to
  back a real market must **not** be filed as a public GitHub issue — public
  disclosure of an exploitable falsely-PASS before markets close is itself
  harmful. Use the confidential channel.
- **Channel (NEEDS SETUP):** a dedicated security contact — e.g. a monitored
  `security@…` mailbox or a GitHub **private security advisory** on
  `toon-protocol/capability-market` (GitHub → Security → Advisories → *Report a
  vulnerability*). **This must be created and staffed before the window opens.**
- **Report contents:** the predicate + file, the proposition clause or binding
  rule broken, a concrete reproducing input (bytes or a failing test), expected
  vs actual verdict, and your assessment of severity.
- **Coordinated disclosure:** the team triages, fixes, re-reviews, and (for the
  mint-ready matmul) re-freezes the image ID before any public write-up. Reporters
  are credited unless they ask otherwise.

---

## Open dependencies (blockers before this program goes live)

1. **Reward pool funding** — treasury allocation TBD (#122 precondition).
2. **Private disclosure channel** — security mailbox / GitHub advisory intake to
   be created and staffed.
3. **Guest compilation + image-ID freeze for sat and difftest** — required before
   those two can be exposed under bounty (they are logic-ready only today).
4. **Two human sign-offs per predicate** — see the review packets; the bounty
   window follows sign-off, it does not replace it.
