// Headless self-check: no r0vm, no network. Proves the JS ports match the Rust
// crates byte-for-byte, so a Kalypso-outsourced seal can be paired with a locally
// reconstructed journal. The golden vector below is the exact output of the
// repo's `e2e-prover --mode dev --rank-bound 49` over the canonical matmul guest
// (image 0x660d47e3…), i.e. produced by `predicates/crates/{journal,matmul}`.

import { strict as assert } from "node:assert";
import { KNOWN_WITNESS, strassen4x4Rank49 } from "../lib/matmul.mjs";
import { predicateJournal, encodeJournal, decodeJournal, sha256 } from "../lib/journal.mjs";

const IMAGE_ID = "660d47e33136b07e362d5efac8669ee3d31603df5aa5c12dba41f91156e8ecff";
const GOLDEN = {
  market_params_hash: "218cd422fe6a50299655006c5c9a13a4a06d5d815f4c929b876885dda1fd4652",
  submission_hash: "89c4eed6b0a7a47a3d855b6e7b174538671866c59af25cd71f978fef369e1ece",
  journal_hex:
    "660d47e33136b07e362d5efac8669ee3d31603df5aa5c12dba41f91156e8ecff" +
    "218cd422fe6a50299655006c5c9a13a4a06d5d815f4c929b876885dda1fd4652" +
    "89c4eed6b0a7a47a3d855b6e7b174538671866c59af25cd71f978fef369e1ece" +
    "01",
};

let n = 0;
const ok = (m) => { console.log(`ok ${++n} - ${m}`); };

// witness shape
assert.equal(strassen4x4Rank49().length, 49, "rank-49 witness has 49 triples");
assert.equal(KNOWN_WITNESS.submission.length, 49 * 6, "submission is 294 bytes");
ok("rank-49 Strassen⊗Strassen witness encodes to 294 bytes");

// hashes match the Rust matmul/journal crates
assert.equal(sha256(KNOWN_WITNESS.marketParams).toString("hex"), GOLDEN.market_params_hash);
assert.equal(sha256(KNOWN_WITNESS.submission).toString("hex"), GOLDEN.submission_hash);
ok("market_params_hash + submission_hash match e2e-prover golden output");

// full journal reconstruction is byte-identical to what the guest commits
const j = predicateJournal(Buffer.from(IMAGE_ID, "hex"), KNOWN_WITNESS.marketParams, KNOWN_WITNESS.submission, true);
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
