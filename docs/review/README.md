# Adversarial review — capability-market launch predicates

This directory holds the materials **two independent human reviewers** read
before any real (value-bearing) capability market opens on a launch predicate.
It is the human sign-off layer required by
[toon-protocol/toon-meta#122](https://github.com/toon-protocol/toon-meta/issues/122).

> **These packets are inputs to review, not a record of it.** Nothing here is a
> sign-off. A predicate is cleared to back a live market only when the sign-off
> blocks at the bottom of its packet carry two independent reviewers' verdicts of
> `APPROVE`, *and* the predicate is mint-ready (see the status table below).

A bug in one of these verifiers is a rigged market with real money at stake, so
the review happens **before** markets open, not after.

---

## 1. The review process

### Roles

- **Author** — wrote the predicate, its tests, and the natural-language
  proposition. Prior agent review has already been performed on each predicate
  (see PRs #1/#2/#3 and the "known edge cases" section of each packet); the
  agent findings are the reviewer's *starting point*, not a substitute for
  independent human judgement.
- **Reviewer A (spec-conformance)** — reads the Rust source against the
  natural-language proposition line by line, hunting for any case where the code
  **accepts** a submission the English rejects, or **rejects** one the English
  accepts. The English↔Rust mapping table in each packet is the spine of this
  pass; verify it, do not trust it.
- **Reviewer B (spec-exploit)** — attempts to *construct* a submission that
  satisfies the code but violates the intent (a "spec exploit"), or one that
  satisfies the intent but the code rejects. Each packet ships a
  **spec-exploit worksheet** of concrete attack angles to work through.

The two reviewers **must be independent of the author and of each other** —
they should not coordinate on findings before both have formed a verdict.
Between them they must exercise every negative vector listed in the packet and
confirm each is rejected.

### What a sign-off asserts

By recording `APPROVE`, a reviewer asserts, to the best of their ability and on
their own name:

1. The natural-language proposition in the packet is the proposition the Rust
   code enforces — **no gap in either direction** that they could find.
2. Every negative vector in the packet is in fact rejected (verdict `false` or a
   clean, non-panicking rejection).
3. They worked the spec-exploit worksheet and found no PASS-the-code /
   violate-the-intent (or FAIL-the-code / satisfy-the-intent) construction that
   is not already documented as *accepted* in the packet.
4. For a **mint-ready** predicate only: the frozen image ID in
   `predicates/ARTIFACTS.json` is the one that will be pinned on-chain.

A sign-off does **not** assert anything about `CapabilityMarket.sol`, the RISC
Zero proving stack, gas, or UX — those are out of scope (see `BUG-BOUNTY.md` and
the non-goals in #122).

### How to record a sign-off

Fill in the sign-off block at the bottom of the predicate's packet in a commit
on a branch named `review/<predicate>-<reviewer>`, opened as a PR against the
predicate's home. Use your real name and an ISO date. Verdict is one of
`APPROVE` / `APPROVE-WITH-NOTES` / `REJECT`. A `REJECT` or any newly found
falsely-PASS / falsely-FAIL must be filed as an issue and (if it is a code
defect) fixed and re-reviewed before that predicate can open.

**Two `APPROVE` (or `APPROVE-WITH-NOTES` whose notes are dispositioned)** from
independent reviewers are required per predicate. A predicate that is *not*
mint-ready can complete logic-level sign-off, but cannot back a live market
until its guest is compiled and its image ID frozen and re-confirmed.

---

## 2. Current predicate status

Honest snapshot as of this packet. **Read this before signing anything — two of
the three predicates are review-ready at the logic level but not yet
mint-ready.**

| Predicate  | Proposition (short)                                   | Verifier logic | Compiled RISC Zero guest | Image ID frozen | Arweave | Mint-ready? | Packet |
|------------|-------------------------------------------------------|:--------------:|:------------------------:|:---------------:|:-------:|:-----------:|--------|
| **matmul** | rank-≤46 bilinear scheme for 4×4 matmul over GF(2)    | ✅ host-lib     | ✅ real guest (`methods/guest`) | ✅ `80db88cd…985b2` | ✅ tx `KRYHAC…WmzU` | **Yes** | [REVIEW-matmul.md](./REVIEW-matmul.md) |
| **sat**    | satisfying assignment for a pinned 3-SAT instance     | ✅ host-lib     | ❌ **none yet**           | ❌ **not frozen** | ❌      | **No — logic only** | [REVIEW-sat.md](./REVIEW-sat.md) |
| **difftest** | input on which two pinned programs diverge           | ✅ host-lib     | ❌ **none yet**           | ❌ **not frozen** | ❌      | **No — logic only** | [REVIEW-difftest.md](./REVIEW-difftest.md) |

What this means concretely:

- **matmul** ships a real compiled RISC Zero guest (`predicates/crates/matmul/methods/guest/src/main.rs`),
  a **frozen canonical image ID** `80db88cd4190c8adf12b58c2aca51812b7a3ca82fa04a0a61c8f91b9dc9985b2`
  (`predicates/ARTIFACTS.json`), the guest ELF (`artifacts/matmul-guest.canonical.bin.gz`),
  a canonical input manifest (`artifacts/matmul-flagship.manifest.bin`), and a
  recorded Arweave upload. It is the only predicate that can back a real market
  once human sign-off lands.
- **sat** and **difftest** ship as **host-testable verifier libraries plus the
  full manifest layer** (`check` / `check_manifest` / `evaluate`), with adversarial
  test suites and pinned launch fixtures — but **no `methods/guest` shell and no
  frozen image ID**. Their `src/lib.rs` is the money-critical logic and is
  review-ready today; but nothing can be minted against them until a guest is
  compiled (following `predicates/crates/template`) and its image ID frozen the
  way matmul's was. Do not sign these off as *mint-ready*; sign them off as
  *logic-ready* and record the open dependency.

---

## 3. The manifest / journal binding every predicate shares

All three predicates share one envelope, so review each predicate's *logic* but
review the binding **once, here**.

### Journal (`predicates/crates/journal`, `journal-v1`)

Every guest commits exactly one 97-byte journal via
`env::commit_slice(&journal.encode())` (never `env::commit`, which would serde-
encode different bytes):

```
offset  size  field                 encoding
0       32    image_id              opaque digest, copied verbatim
32      32    market_params_hash    opaque digest (= sha256 of the manifest bytes)
64      32    submission_hash       sha256 of the raw submission bytes
96      1     verdict               0x00 = false, 0x01 = true; any other byte is a decode error
```

- No multi-byte integers → endianness never applies; byte `i` of each Rust
  `[u8; 32]` is byte `i` of the Solidity `bytes32` (no swapping).
- `Journal::digest()` = `sha256(encode())` — the exact value
  `CapabilityMarket.sol` hands the RISC Zero verifier.
- **No in-band version tag**: the on-chain-pinned `imageId` transitively pins the
  exact encoder; a layout change means new images and a migration.
- `image_id` is a guest *input* (a guest can't hash itself), committed verbatim;
  its integrity is enforced on-chain (the verifier checks the seal against the
  market's pinned image ID and the contract requires `journal.imageId ==
  market.imageId`). A prover lying about `image_id` produces a journal the
  contract rejects.
- Golden conformance vectors: `predicates/crates/journal/tests/golden_journal_vectors.json`
  (the Solidity decoder in PR #2 embeds these verbatim, incl. the 96-byte /
  98-byte / legacy-128-byte-`abi.encode` / verdict-`0x02` rejection cases).

### Manifest (`predicates/crates/manifest`, `manifest-v1` = magic `TMF1`)

`marketParamsHash = sha256(canonical manifest bytes)`, **NOT** `sha256(raw
params)` — this resolves capability-market#4 per toon-meta#121. The same bytes
are hashed off-chain (authoring tooling → `createMarket`) and re-hashed inside
the guest, so they must agree byte-for-byte.

- Length-prefixed binary TLV: `magic "TMF1" | entry_count u16 LE | entries`.
  Each entry: `kind u8 | name_len u16 LE | name | data_len u32 LE | data`.
- Three entry kinds: **HASH** (32-byte content address of external bytes, guest
  must verify `sha256(bytes)==digest` before trusting), **VALUE** (literal pinned
  parameter embedded inline), **SLOT** (the one late-bound `submission`, zero data).
- **Canonical on parse** — `parse()` rejects: bad magic; `entry_count == 0` or
  mismatched; entries not in *strictly ascending* name order (also forbids
  duplicate names); unknown kind; HASH not 32 bytes / SLOT with data; empty or
  non-UTF-8 name; trailing bytes; truncation. Exactly one byte string decodes to
  a given manifest ⇒ no two byte strings share a `marketParamsHash` preimage.
- The three launch predicates embed their parameters as **VALUE** entries (not
  HASH), so their guest reads only `(image_id, manifest_bytes, submission)` — no
  side-channel input. Each pins a `frozen_clock` VALUE (deadline literal, **audit
  only** — see the note below), a predicate-specific parameter VALUE, and the
  `submission` SLOT.
- Golden vectors: `predicates/crates/manifest/tests/golden_manifest_vectors.json`.

### One cross-cutting note reviewers must carry (raised in PR #3 review, still open)

The verdict every launch guest commits is **time-independent**: the guest hashes
`frozen_clock` into `market_params_hash` (so the deadline is *pinned and
auditable*) but **does not itself enforce the deadline**. The "by date D" clause
of every proposition is enforced by the `CapabilityMarket.sol` commit/reveal
windows, not by the predicate. Reviewers of the *predicate* should confirm this
is true (the guest never branches on the clock) and treat the deadline as a
contract-side obligation. (The `frozen_clock` in `ARTIFACTS.json`'s example is
`1735689600` = 2025-01-01; the flagship market's real deadline is a `createMarket`
parameter.) This is a **binding-definition** concern, not a predicate-logic
defect. See each packet's edge-case list.
