// Minimal, self-contained port of the canonical capability-market journal
// (`journal-v1`, toon-meta#121). The BYTE-FOR-BYTE authority is the Rust crate
// at `predicates/crates/journal/` (read-only referenced, never modified here) and
// `CapabilityMarket.sol::decodeJournal`. This file exists so the Kalypso path can
// RECONSTRUCT the exact 97-byte journal a Kalypso-hosted RISC Zero prover would
// commit, without depending on the Rust workspace.
//
// Layout (97 bytes, tightly packed):
//   [0..32)  image_id            — opaque 32-byte digest, copied verbatim
//   [32..64) market_params_hash  — sha256(market_params)
//   [64..96) submission_hash     — sha256(submission)
//   [96]     verdict             — 0x00 false / 0x01 true
//
// The journal is fully DETERMINED by (image_id, market_params, submission,
// verdict). That determinism is the load-bearing property for the Kalypso path:
// a marketplace prover only needs to return the risc0 seal; we can regenerate the
// journal locally and the seal binds to sha256(journal). Verified against
// predicates/crates/journal golden vectors — see README §"Journal reconstruction".

import { createHash } from "node:crypto";

export const ENCODED_LEN = 97;
export const ENCODING_VERSION = 1;

/** sha256(bytes) -> 32-byte Buffer. Matches the Rust `journal::sha256`. */
export function sha256(bytes) {
  return createHash("sha256").update(bytes).digest();
}

function as32(name, buf) {
  if (buf.length !== 32) throw new Error(`${name} must be 32 bytes, got ${buf.length}`);
  return buf;
}

/**
 * Assemble the canonical journal the guest commits.
 * @param {Buffer} imageId       32-byte RISC Zero image ID.
 * @param {Buffer} marketParams  raw market-params bytes (hashed here).
 * @param {Buffer} submission    raw submission bytes (hashed here).
 * @param {boolean} verdict      predicate verdict.
 * @returns {{imageId:Buffer, marketParamsHash:Buffer, submissionHash:Buffer, verdict:boolean}}
 */
export function predicateJournal(imageId, marketParams, submission, verdict) {
  return {
    imageId: as32("image_id", imageId),
    marketParamsHash: sha256(marketParams),
    submissionHash: sha256(submission),
    verdict: !!verdict,
  };
}

/** Serialize to the canonical 97-byte `journal-v1` layout. */
export function encodeJournal(j) {
  const out = Buffer.alloc(ENCODED_LEN);
  as32("image_id", j.imageId).copy(out, 0);
  as32("market_params_hash", j.marketParamsHash).copy(out, 32);
  as32("submission_hash", j.submissionHash).copy(out, 64);
  out[96] = j.verdict ? 0x01 : 0x00;
  return out;
}

/** Strict decode — mirrors Rust `Journal::decode` / Solidity `decodeJournal`. */
export function decodeJournal(bytes) {
  if (bytes.length !== ENCODED_LEN) throw new Error(`journal must be ${ENCODED_LEN} bytes, got ${bytes.length}`);
  const v = bytes[96];
  if (v !== 0x00 && v !== 0x01) throw new Error(`verdict byte must be 0x00 or 0x01, got 0x${v.toString(16)}`);
  return {
    imageId: Buffer.from(bytes.subarray(0, 32)),
    marketParamsHash: Buffer.from(bytes.subarray(32, 64)),
    submissionHash: Buffer.from(bytes.subarray(64, 96)),
    verdict: v === 0x01,
  };
}

/** sha256(encode(journal)) — the digest CapabilityMarket.sol hands the verifier. */
export function journalDigest(j) {
  return sha256(encodeJournal(j));
}
