// Headless self-check: no r0vm, no network. Proves the JS ports match the Rust
// crates byte-for-byte, so a Kalypso-outsourced seal can be paired with a locally
// reconstructed journal. The golden vectors below are the exact output of the
// repo's Rust crates: `predicates/crates/manifest` (input manifest, toon-meta#121)
// and `predicates/crates/{journal,matmul}` over the canonical matmul guest
// (image 0x80db88cd…, predicates/ARTIFACTS.json). market_params_hash is
// sha256(canonical manifest-v1 bytes), NOT sha256(raw params) — capability-market#4
// resolved in favour of the manifest.

import { strict as assert } from "node:assert";
import {
  KNOWN_WITNESS,
  strassen4x4Rank49,
  encodeMatmulManifest,
  FLAGSHIP_FROZEN_CLOCK,
} from "../lib/matmul.mjs";
import { predicateJournal, encodeJournal, decodeJournal, sha256 } from "../lib/journal.mjs";

const IMAGE_ID = "80db88cd4190c8adf12b58c2aca51812b7a3ca82fa04a0a61c8f91b9dc9985b2";

// Canonical manifest-v1 golden vectors (frozen_clock = 1735689600), authoritative
// output of `predicates/crates/manifest` (matmul::encode_manifest → manifest::hash).
// rank46 also appears verbatim in predicates/crates/manifest/tests/golden_manifest_vectors.json
// and predicates/ARTIFACTS.json (the flagship rank_bound=46 market).
const MANIFEST_GOLDEN = {
  46: {
    encoded: "544d46310300020c0066726f7a656e5f636c6f636b080000008085746700000000020d006d61726b65745f706172616d7320000000000000000000000000000000000000000000000000000000000000000000002e030a007375626d697373696f6e00000000",
    sha256: "0a029dce586c51f082c6b7e654926602e0602a3b32f7b3d11e9dd6c4cfa6ee0e",
  },
  49: {
    encoded: "544d46310300020c0066726f7a656e5f636c6f636b080000008085746700000000020d006d61726b65745f706172616d73200000000000000000000000000000000000000000000000000000000000000000000031030a007375626d697373696f6e00000000",
    sha256: "619ab7e84a9174861a1c608734b81b4faec3b9e6f695d0e42b23fcedd5b7d710",
  },
};

const GOLDEN = {
  // market_params_hash = sha256(manifest-v1 bytes for rank 49) — the #4 binding.
  market_params_hash: MANIFEST_GOLDEN[49].sha256,
  submission_hash: "89c4eed6b0a7a47a3d855b6e7b174538671866c59af25cd71f978fef369e1ece",
  journal_hex:
    IMAGE_ID +
    MANIFEST_GOLDEN[49].sha256 +
    "89c4eed6b0a7a47a3d855b6e7b174538671866c59af25cd71f978fef369e1ece" +
    "01",
};

let n = 0;
const ok = (m) => { console.log(`ok ${++n} - ${m}`); };

// witness shape
assert.equal(strassen4x4Rank49().length, 49, "rank-49 witness has 49 triples");
assert.equal(KNOWN_WITNESS.submission.length, 49 * 6, "submission is 294 bytes");
ok("rank-49 Strassen⊗Strassen witness encodes to 294 bytes");

// manifest-v1 encoder matches the Rust `manifest` crate byte-for-byte for both
// the flagship bound (46) and the witness bound (49). This is the load-bearing
// cross-check: same bytes + same sha256 ⇒ market_params_hash agrees on-chain.
for (const bound of [46, 49]) {
  const enc = encodeMatmulManifest(bound, FLAGSHIP_FROZEN_CLOCK);
  assert.equal(enc.toString("hex"), MANIFEST_GOLDEN[bound].encoded, `manifest bytes @bound ${bound}`);
  assert.equal(sha256(enc).toString("hex"), MANIFEST_GOLDEN[bound].sha256, `manifest sha256 @bound ${bound}`);
}
ok("manifest-v1 encode + sha256 match the Rust manifest crate for rank-bound 46 and 49");

// the witness's manifest is the rank-49 one
assert.equal(KNOWN_WITNESS.manifest.toString("hex"), MANIFEST_GOLDEN[49].encoded, "KNOWN_WITNESS.manifest == rank-49 manifest");

// hashes match the Rust matmul/journal crates
assert.equal(sha256(KNOWN_WITNESS.manifest).toString("hex"), GOLDEN.market_params_hash);
assert.equal(sha256(KNOWN_WITNESS.submission).toString("hex"), GOLDEN.submission_hash);
ok("market_params_hash = sha256(manifest) + submission_hash match the Rust crates");

// full journal reconstruction is byte-identical to what the guest commits
const j = predicateJournal(Buffer.from(IMAGE_ID, "hex"), KNOWN_WITNESS.manifest, KNOWN_WITNESS.submission, true);
const enc = encodeJournal(j);
assert.equal(enc.length, 97, "journal is 97 bytes");
assert.equal(enc.toString("hex"), GOLDEN.journal_hex, "reconstructed journal == guest journal");
ok("reconstructed 97-byte journal is byte-identical to the guest's committed journal");

// strict decode round-trips + rejects malformed
const back = decodeJournal(enc);
assert.equal(back.verdict, true);
assert.equal(back.imageId.toString("hex"), IMAGE_ID);
assert.throws(() => decodeJournal(enc.subarray(0, 96)), /97 bytes/);
const bad = Buffer.from(enc); bad[96] = 0x02;
assert.throws(() => decodeJournal(bad), /verdict byte/);
ok("strict journal decode round-trips and rejects malformed length/verdict");

console.log(`\n1..${n}\n# all self-checks passed`);
