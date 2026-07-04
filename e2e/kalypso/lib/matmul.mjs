// Minimal port of the matmul predicate's on-the-wire encodings, enough to
// assemble a proof-request payload for the flagship matmul market. Byte-for-byte
// authority is `predicates/crates/matmul/src/lib.rs` and
// `predicates/crates/manifest/src/lib.rs` (read-only referenced).
//
//   market-params-v1 : 32-byte big-endian uint (rank bound), top 28 bytes zero
//                       (== Solidity abi.encode(uint256(bound))).
//   submission-v1    : r triples × 6 bytes: u(u16 BE) || v(u16 BE) || w(u16 BE).
//   manifest-v1      : canonical TLV input manifest (toon-meta#121). The guest
//                      reads (image_id, manifest_bytes, submission) and commits
//                      market_params_hash = sha256(manifest_bytes) — NOT
//                      sha256(raw params). The rank bound rides as the
//                      `market_params` VALUE entry; the deadline as `frozen_clock`;
//                      the late-bound submission as a `submission` SLOT.
//
// We ship exactly one known witness: the rank-49 Strassen⊗Strassen scheme, the
// canonical valid submission at rank-bound 49 (the same witness e2e-prover uses).

/** market-params-v1: rank bound as a 32-byte big-endian uint. */
export function encodeMarketParams(bound) {
  if (!Number.isInteger(bound) || bound < 0 || bound > 0xffffffff) {
    throw new Error(`rank bound must be a u32, got ${bound}`);
  }
  const out = Buffer.alloc(32);
  out.writeUInt32BE(bound >>> 0, 28);
  return out;
}

/** submission-v1: concat of {u,v,w} u16-BE triples. */
export function encodeScheme(triples) {
  const out = Buffer.alloc(triples.length * 6);
  triples.forEach(([u, v, w], i) => {
    out.writeUInt16BE(u & 0xffff, i * 6);
    out.writeUInt16BE(v & 0xffff, i * 6 + 2);
    out.writeUInt16BE(w & 0xffff, i * 6 + 4);
  });
  return out;
}

// Strassen's rank-7 2×2 scheme over GF(2) (signs vanish mod 2). Bit j ↔ (j/2, j%2).
const STRASSEN_2X2 = [
  [0b1001, 0b1001, 0b1001],
  [0b1100, 0b0001, 0b1100],
  [0b0001, 0b1010, 0b1010],
  [0b1000, 0b0101, 0b0101],
  [0b0011, 0b1000, 0b0011],
  [0b0101, 0b0011, 0b1000],
  [0b1010, 0b1100, 0b0001],
];

// Kronecker-lift a 2×2-level mask pair to a 4×4-level 16-bit mask.
function tensor(outer, inner) {
  let out = 0;
  for (let ob = 0; ob < 4; ob++) {
    if ((outer >> ob) & 1) {
      const br = ob >> 1, bc = ob & 1;
      for (let ib = 0; ib < 4; ib++) {
        if ((inner >> ib) & 1) {
          const ir = ib >> 1, ic = ib & 1;
          out |= 1 << ((2 * br + ir) * 4 + (2 * bc + ic));
        }
      }
    }
  }
  return out & 0xffff;
}

/** The rank-49 witness: Strassen recursed on itself once (7×7 = 49 products). */
export function strassen4x4Rank49() {
  const out = [];
  for (const [uo, vo, wo] of STRASSEN_2X2) {
    for (const [ui, vi, wi] of STRASSEN_2X2) {
      out.push([tensor(uo, ui), tensor(vo, vi), tensor(wo, wi)]);
    }
  }
  return out; // length 49
}

// ---------------------------------------------------------------------------
// manifest-v1 canonical input manifest (toon-meta#121, resolves capability-market#4)
// Port of `predicates/crates/manifest/src/lib.rs`. All multi-byte integers are
// LITTLE-endian:
//   magic "TMF1" || entry_count(u16) || entries…
//   entry = kind(u8) || name_len(u16) || name || data_len(u32) || data
//     HASH(0x01): data_len == 32   VALUE(0x02): literal bytes   SLOT(0x03): data_len == 0
// Entries are emitted in STRICTLY ASCENDING name order (bytewise); names unique,
// 1..=65535 bytes. sha256(encode) is the market's marketParamsHash.
// ---------------------------------------------------------------------------

export const MANIFEST_MAGIC = Buffer.from("TMF1", "ascii");
export const MANIFEST_KIND = { HASH: 0x01, VALUE: 0x02, SLOT: 0x03 };

// Matmul manifest entry names (must match matmul::MANIFEST_*).
export const MANIFEST_MARKET_PARAMS = "market_params";
export const MANIFEST_FROZEN_CLOCK = "frozen_clock";
export const MANIFEST_SUBMISSION = "submission";

// The flagship market's pinned deadline literal (e2e-prover's default --frozen-clock).
export const FLAGSHIP_FROZEN_CLOCK = 1735689600;

/**
 * Encode canonical `manifest-v1` bytes from entries.
 * @param {Array<{name:string, kind:number, data?:Buffer}>} entries
 *   kind VALUE/HASH carry `data`; SLOT carries none. HASH data must be 32 bytes.
 * @returns {Buffer}
 */
export function encodeManifest(entries) {
  if (!Array.isArray(entries) || entries.length === 0) {
    throw new Error("manifest must have at least one entry");
  }
  for (const e of entries) {
    const nlen = Buffer.byteLength(e.name, "utf8");
    if (nlen < 1 || nlen > 0xffff) throw new Error(`entry name ${JSON.stringify(e.name)} must be 1..=65535 bytes`);
    if (e.kind === MANIFEST_KIND.HASH && (!e.data || e.data.length !== 32)) {
      throw new Error(`HASH entry ${JSON.stringify(e.name)} must carry 32 bytes`);
    }
  }
  // Canonical order: strictly ascending by UTF-8 name bytes (also rejects dups).
  const sorted = [...entries].sort((a, b) =>
    Buffer.from(a.name, "utf8").compare(Buffer.from(b.name, "utf8")));
  for (let i = 1; i < sorted.length; i++) {
    if (sorted[i].name === sorted[i - 1].name) throw new Error(`duplicate entry name ${JSON.stringify(sorted[i].name)}`);
  }

  const chunks = [MANIFEST_MAGIC];
  const count = Buffer.alloc(2); count.writeUInt16LE(sorted.length, 0); chunks.push(count);
  for (const e of sorted) {
    chunks.push(Buffer.from([e.kind]));
    const name = Buffer.from(e.name, "utf8");
    const nl = Buffer.alloc(2); nl.writeUInt16LE(name.length, 0); chunks.push(nl, name);
    if (e.kind === MANIFEST_KIND.SLOT) {
      chunks.push(Buffer.alloc(4)); // data_len = 0, no data
    } else {
      const data = e.data ?? Buffer.alloc(0);
      const dl = Buffer.alloc(4); dl.writeUInt32LE(data.length, 0); chunks.push(dl, data);
    }
  }
  return Buffer.concat(chunks);
}

/**
 * Build the canonical matmul input manifest for a market — the exact bytes
 * `matmul::encode_manifest(rankBound, frozenClock)` produces. sha256 of the
 * result is the market's marketParamsHash committed as the journal's
 * market_params_hash.
 */
export function encodeMatmulManifest(rankBound, frozenClock = FLAGSHIP_FROZEN_CLOCK) {
  const clock = Buffer.alloc(8);
  clock.writeBigUInt64LE(BigInt(frozenClock), 0);
  return encodeManifest([
    { name: MANIFEST_MARKET_PARAMS, kind: MANIFEST_KIND.VALUE, data: encodeMarketParams(rankBound) },
    { name: MANIFEST_FROZEN_CLOCK, kind: MANIFEST_KIND.VALUE, data: clock },
    { name: MANIFEST_SUBMISSION, kind: MANIFEST_KIND.SLOT },
  ]);
}

/** Canonical flagship rank-49 witness + its input manifest, for a proof request. */
export const KNOWN_WITNESS = {
  rankBound: 49,
  frozenClock: FLAGSHIP_FROZEN_CLOCK,
  // Raw 32-byte market-params (the VALUE embedded in the manifest; kept for reference).
  get marketParams() { return encodeMarketParams(49); },
  // The canonical manifest bytes the guest reads and hashes into market_params_hash.
  get manifest() { return encodeMatmulManifest(49, FLAGSHIP_FROZEN_CLOCK); },
  get submission() { return encodeScheme(strassen4x4Rank49()); },
};
