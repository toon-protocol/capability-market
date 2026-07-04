// Minimal port of the matmul predicate's on-the-wire encodings, enough to
// assemble a proof-request payload for the flagship matmul market. Byte-for-byte
// authority is `predicates/crates/matmul/src/lib.rs` (read-only referenced).
//
//   market-params-v1 : 32-byte big-endian uint (rank bound), top 28 bytes zero
//                       (== Solidity abi.encode(uint256(bound))).
//   submission-v1    : r triples × 6 bytes: u(u16 BE) || v(u16 BE) || w(u16 BE).
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

/** Canonical flagship image ID + rank-49 witness, encoded for a proof request. */
export const KNOWN_WITNESS = {
  rankBound: 49,
  get marketParams() { return encodeMarketParams(49); },
  get submission() { return encodeScheme(strassen4x4Rank49()); },
};
