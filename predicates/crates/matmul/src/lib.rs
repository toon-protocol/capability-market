//! Flagship capability-market predicate (toon-meta#122):
//!
//! > "A rank-≤46 bilinear scheme for 4×4 matrix multiplication over GF(2),
//! > symbolically verified, is published by 2027-07-01."
//!
//! Given a claimed bilinear scheme `{(u_i, v_i, w_i) for i in 1..=r}`, this
//! verifier symbolically expands `Σ_i w_i (u_i·A)(v_i·B)` as polynomials in
//! the 32 entries of `A` and `B` over GF(2) and checks equality with the 16
//! entries of `AB`, plus `r ≤ bound` where `bound` comes from market params
//! (46 for the flagship market).
//!
//! All logic here is pure Rust and host-testable with `cargo test`; the RISC
//! Zero guest (`methods/guest`) is a thin `env::read → check → env::commit`
//! shell over [`check`].
//!
//! # Submission encoding (`matmul-submission-v1`)
//!
//! `r` triples, 6 bytes each, concatenated (`len = 6r`, `r ≥ 1`):
//! `u_i (u16 BE) || v_i (u16 BE) || w_i (u16 BE)`.
//!
//! Each `u16` is a coefficient vector over GF(2). Bit `j` (value `1 << j`)
//! addresses matrix entry `(row, col) = (j / 4, j % 4)`, row-major:
//! `u_i` over the 16 entries of `A`, `v_i` over `B`, `w_i` over `C = AB`.
//! Big-endian to match EVM-side tooling that assembles submissions.
//!
//! # Market params encoding (`matmul-market-params-v1`)
//!
//! Exactly 32 bytes: the rank bound as a big-endian unsigned integer (i.e.
//! Solidity `abi.encode(uint256(bound))`). The top 28 bytes must be zero
//! (bounds beyond `u32` are meaningless for a 16×16×16 tensor and are
//! rejected as malformed). The flagship market pins
//! [`FLAGSHIP_RANK_BOUND`] = 46.
//!
//! # What the verifier rejects
//!
//! Beyond polynomial-identity failure and rank overrun, structurally
//! degenerate schemes are rejected even when the identity holds:
//!
//! - **zero vectors** — a triple with `u_i = 0`, `v_i = 0` or `w_i = 0`
//!   contributes nothing and exists only to misstate the scheme;
//! - **duplicate products** — two triples with the same `(u_i, v_i)`. Over
//!   GF(2) their contributions can cancel, so without this check a scheme
//!   could smuggle in cancelling padding and the stated `r` would not count
//!   `r` genuinely distinct rank-1 products.
//!
//! The English proposition says "a rank-≤46 bilinear scheme": `r` must be
//! the number of distinct, non-trivial products actually used.

use journal::Journal;

/// The flagship market's pinned rank bound.
pub const FLAGSHIP_RANK_BOUND: u32 = 46;

/// Number of matrix entries (4×4).
pub const ENTRIES: usize = 16;

/// One bilinear product `(u·A)(v·B)` fanned out to outputs `w`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triple {
    /// GF(2) coefficients over the 16 entries of `A` (bit `j` ↔ `A[j/4][j%4]`).
    pub u: u16,
    /// GF(2) coefficients over the 16 entries of `B`.
    pub v: u16,
    /// GF(2) coefficients over the 16 entries of `C = AB` this product feeds.
    pub w: u16,
}

/// Why a scheme was rejected. The guest maps any of these to `verdict: false`;
/// the reasons exist for tests and authoring tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// Submission bytes are not a well-formed `matmul-submission-v1` payload.
    MalformedSubmission,
    /// Market params are not a well-formed `matmul-market-params-v1` payload.
    MalformedMarketParams,
    /// Scheme has no products.
    EmptyScheme,
    /// A triple has `u = 0`, `v = 0` or `w = 0`.
    DegenerateProduct { index: usize },
    /// `r` exceeds the market's rank bound.
    RankExceeded { rank: usize, bound: u32 },
    /// Two triples share the same `(u, v)` product.
    DuplicateProduct { first: usize, second: usize },
    /// The symbolic expansion does not equal `AB`.
    NotMatmul { output: usize },
}

/// Verify a parsed scheme against a rank bound.
///
/// Check order: empty → degenerate → rank → duplicate → polynomial identity.
pub fn verify_scheme(triples: &[Triple], bound: u32) -> Result<(), Reject> {
    if triples.is_empty() {
        return Err(Reject::EmptyScheme);
    }
    for (i, t) in triples.iter().enumerate() {
        if t.u == 0 || t.v == 0 || t.w == 0 {
            return Err(Reject::DegenerateProduct { index: i });
        }
    }
    if triples.len() as u64 > bound as u64 {
        return Err(Reject::RankExceeded {
            rank: triples.len(),
            bound,
        });
    }
    // r ≤ bound ≤ u32::MAX here, and in practice tiny; O(r²) scan is fine
    // (and cheap in-circuit for r ≤ 64-ish schemes).
    for i in 0..triples.len() {
        for j in (i + 1)..triples.len() {
            if triples[i].u == triples[j].u && triples[i].v == triples[j].v {
                return Err(Reject::DuplicateProduct { first: i, second: j });
            }
        }
    }
    check_identity(triples)
}

/// Symbolic polynomial-identity check over GF(2).
///
/// For each output entry `C[p][q]` we accumulate the 16×16 GF(2) coefficient
/// matrix `M` of the bilinear form `Σ_{i : w_i ∋ (p,q)} (u_i·A)(v_i·B)`,
/// where `M[j][k]` is the coefficient of the monomial `A_j · B_k` (bit `k` of
/// row `j`). Expanding `(u_i·A)(v_i·B) = Σ_{j∈u_i} Σ_{k∈v_i} A_j B_k`, each
/// contributing triple XORs `v_i` into every row `j ∈ u_i`. The target is
/// `(AB)[p][q] = Σ_t A[p][t] B[t][q]`: coefficient 1 exactly at
/// `(j, k) = (4p + t, 4t + q)` for `t = 0..4`, 0 elsewhere.
fn check_identity(triples: &[Triple]) -> Result<(), Reject> {
    for o in 0..ENTRIES {
        let (p, q) = (o / 4, o % 4);
        let mut m = [0u16; ENTRIES]; // m[j]: bitmask over B-entry index k
        for t in triples {
            if (t.w >> o) & 1 == 1 {
                for j in 0..ENTRIES {
                    if (t.u >> j) & 1 == 1 {
                        m[j] ^= t.v;
                    }
                }
            }
        }
        let mut target = [0u16; ENTRIES];
        for t in 0..4 {
            target[4 * p + t] |= 1 << (4 * t + q);
        }
        if m != target {
            return Err(Reject::NotMatmul { output: o });
        }
    }
    Ok(())
}

/// Parse `matmul-submission-v1` bytes.
pub fn decode_scheme(submission: &[u8]) -> Result<Vec<Triple>, Reject> {
    if submission.is_empty() || submission.len() % 6 != 0 {
        return Err(Reject::MalformedSubmission);
    }
    Ok(submission
        .chunks_exact(6)
        .map(|c| Triple {
            u: u16::from_be_bytes([c[0], c[1]]),
            v: u16::from_be_bytes([c[2], c[3]]),
            w: u16::from_be_bytes([c[4], c[5]]),
        })
        .collect())
}

/// Serialize a scheme to `matmul-submission-v1` bytes.
pub fn encode_scheme(triples: &[Triple]) -> Vec<u8> {
    let mut out = Vec::with_capacity(triples.len() * 6);
    for t in triples {
        out.extend_from_slice(&t.u.to_be_bytes());
        out.extend_from_slice(&t.v.to_be_bytes());
        out.extend_from_slice(&t.w.to_be_bytes());
    }
    out
}

/// Parse `matmul-market-params-v1` bytes (32-byte BE uint, top 28 bytes zero).
pub fn decode_market_params(market_params: &[u8]) -> Result<u32, Reject> {
    let bytes: &[u8; 32] = market_params
        .try_into()
        .map_err(|_| Reject::MalformedMarketParams)?;
    if bytes[..28].iter().any(|&b| b != 0) {
        return Err(Reject::MalformedMarketParams);
    }
    Ok(u32::from_be_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]))
}

/// Serialize a rank bound to `matmul-market-params-v1` bytes
/// (`abi.encode(uint256(bound))`).
pub fn encode_market_params(bound: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[28..32].copy_from_slice(&bound.to_be_bytes());
    out
}

/// The predicate entry point the guest calls: raw bytes in, verdict out.
/// Any malformation or rejection is `false` — the proof then attests that
/// this submission does NOT satisfy the proposition.
pub fn check(market_params: &[u8], submission: &[u8]) -> bool {
    let Ok(bound) = decode_market_params(market_params) else {
        return false;
    };
    let Ok(triples) = decode_scheme(submission) else {
        return false;
    };
    verify_scheme(&triples, bound).is_ok()
}

/// Full guest computation, host-callable for tests: parse nothing, judge the
/// raw bytes, and assemble the journal exactly as the guest commits it.
pub fn evaluate(image_id: [u8; 32], market_params: &[u8], submission: &[u8]) -> Journal {
    let verdict = check(market_params, submission);
    journal::predicate_journal(image_id, market_params, submission, verdict)
}

/// Reference schemes for tests and adversarial review (NOT witnesses for the
/// flagship market — both exceed rank 46).
pub mod schemes {
    use super::Triple;

    /// Strassen's rank-7 scheme for 2×2 matmul, valid over GF(2) (all signs
    /// vanish mod 2). Bit `j` of each mask addresses entry `(j / 2, j % 2)`
    /// of the 2×2 matrix.
    ///
    /// M1=(A11+A22)(B11+B22)→C11,C22; M2=(A21+A22)B11→C21,C22;
    /// M3=A11(B12+B22)→C12,C22; M4=A22(B21+B11)→C11,C21;
    /// M5=(A11+A12)B22→C11,C12; M6=(A21+A11)(B11+B12)→C22;
    /// M7=(A12+A22)(B21+B22)→C11.
    pub const STRASSEN_2X2: [(u8, u8, u8); 7] = [
        (0b1001, 0b1001, 0b1001),
        (0b1100, 0b0001, 0b1100),
        (0b0001, 0b1010, 0b1010),
        (0b1000, 0b0101, 0b0101),
        (0b0011, 0b1000, 0b0011),
        (0b0101, 0b0011, 0b1000),
        (0b1010, 0b1100, 0b0001),
    ];

    /// Kronecker-lift a 2×2-level mask pair (outer over blocks, inner within
    /// a block) to a 4×4-level 16-bit mask: entry `(2·br + ir, 2·bc + ic)` is
    /// set iff outer bit `(br, bc)` and inner bit `(ir, ic)` are both set.
    fn tensor(outer: u8, inner: u8) -> u16 {
        let mut out = 0u16;
        for ob in 0..4 {
            if (outer >> ob) & 1 == 1 {
                let (br, bc) = (ob / 2, ob % 2);
                for ib in 0..4 {
                    if (inner >> ib) & 1 == 1 {
                        let (ir, ic) = (ib / 2, ib % 2);
                        out |= 1 << ((2 * br + ir) * 4 + (2 * bc + ic));
                    }
                }
            }
        }
        out
    }

    /// The rank-49 scheme for 4×4 matmul over GF(2): Strassen recursed on
    /// itself once (Strassen ⊗ Strassen, 7 × 7 = 49 products).
    pub fn strassen_4x4_rank49() -> Vec<Triple> {
        let mut out = Vec::with_capacity(49);
        for &(uo, vo, wo) in &STRASSEN_2X2 {
            for &(ui, vi, wi) in &STRASSEN_2X2 {
                out.push(Triple {
                    u: tensor(uo, ui),
                    v: tensor(vo, vi),
                    w: tensor(wo, wi),
                });
            }
        }
        out
    }

    /// The trivial rank-64 scheme: one product `A[p][k]·B[k][q]` per term.
    pub fn naive_4x4_rank64() -> Vec<Triple> {
        let mut out = Vec::with_capacity(64);
        for p in 0..4u16 {
            for q in 0..4u16 {
                for k in 0..4u16 {
                    out.push(Triple {
                        u: 1 << (p * 4 + k),
                        v: 1 << (k * 4 + q),
                        w: 1 << (p * 4 + q),
                    });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::schemes::{naive_4x4_rank64, strassen_4x4_rank49};
    use super::*;

    /// Brute-force ground truth: multiply concrete matrices with the claimed
    /// scheme and with the definition, over all-random samples. (The symbolic
    /// check is the verifier; this cross-checks the reference schemes really
    /// compute matmul on concrete inputs too.)
    fn scheme_multiplies(triples: &[Triple], a: u16, b: u16) -> u16 {
        // a, b: 16-bit row-major GF(2) matrices.
        let dot = |mask: u16, m: u16| ((mask & m).count_ones() & 1) as u16;
        let mut c = 0u16;
        for t in triples {
            let prod = dot(t.u, a) & dot(t.v, b);
            if prod == 1 {
                c ^= t.w;
            }
        }
        c
    }

    fn true_matmul(a: u16, b: u16) -> u16 {
        let get = |m: u16, r: usize, c: usize| (m >> (r * 4 + c)) & 1;
        let mut out = 0u16;
        for p in 0..4 {
            for q in 0..4 {
                let mut s = 0u16;
                for k in 0..4 {
                    s ^= get(a, p, k) & get(b, k, q);
                }
                out |= s << (p * 4 + q);
            }
        }
        out
    }

    fn xorshift(state: &mut u32) -> u16 {
        *state ^= *state << 13;
        *state ^= *state >> 17;
        *state ^= *state << 5;
        (*state & 0xffff) as u16
    }

    #[test]
    fn strassen49_is_valid_at_bound_49() {
        let s = strassen_4x4_rank49();
        assert_eq!(s.len(), 49);
        assert_eq!(verify_scheme(&s, 49), Ok(()));
    }

    #[test]
    fn naive64_is_valid_at_bound_64() {
        let s = naive_4x4_rank64();
        assert_eq!(s.len(), 64);
        assert_eq!(verify_scheme(&s, 64), Ok(()));
    }

    #[test]
    fn reference_schemes_multiply_concrete_matrices() {
        let mut rng = 0xdeadbeefu32;
        for scheme in [strassen_4x4_rank49(), naive_4x4_rank64()] {
            for _ in 0..500 {
                let (a, b) = (xorshift(&mut rng), xorshift(&mut rng));
                assert_eq!(scheme_multiplies(&scheme, a, b), true_matmul(a, b));
            }
        }
    }

    #[test]
    fn flipped_coefficient_fails_identity() {
        // Flip a single u-bit in each position of the first triple in turn:
        // every such corruption must break the polynomial identity.
        let base = strassen_4x4_rank49();
        for bit in 0..16 {
            let mut s = base.clone();
            s[0].u ^= 1 << bit;
            if s[0].u == 0 {
                continue; // becomes a degenerate triple, covered elsewhere
            }
            match verify_scheme(&s, 49) {
                Err(Reject::NotMatmul { .. }) => {}
                other => panic!("u-bit {bit}: expected NotMatmul, got {other:?}"),
            }
        }
        // And one flipped w-bit.
        let mut s = base.clone();
        s[10].w ^= 1;
        assert!(matches!(
            verify_scheme(&s, 49),
            Err(Reject::NotMatmul { .. })
        ));
    }

    #[test]
    fn valid_scheme_fails_flagship_bound_46() {
        let s = strassen_4x4_rank49();
        assert_eq!(
            verify_scheme(&s, FLAGSHIP_RANK_BOUND),
            Err(Reject::RankExceeded {
                rank: 49,
                bound: 46
            })
        );
    }

    #[test]
    fn duplicate_product_rejected_even_when_identity_holds() {
        // Append the SAME triple twice: the two copies cancel over GF(2), so
        // the polynomial identity still holds and r = 51 ≤ 64 — only the
        // duplicate check catches the padding.
        let mut s = strassen_4x4_rank49();
        let pad = s[3];
        s.push(pad);
        s.push(pad);
        assert_eq!(
            verify_scheme(&s, 64),
            Err(Reject::DuplicateProduct { first: 3, second: 49 })
        );
    }

    #[test]
    fn degenerate_zero_vectors_rejected() {
        for zeroed in [
            Triple { u: 0, v: 0b1, w: 0b1 },
            Triple { u: 0b1, v: 0, w: 0b1 },
            Triple { u: 0b1, v: 0b1, w: 0 },
        ] {
            let mut s = strassen_4x4_rank49();
            s.push(zeroed);
            assert_eq!(
                verify_scheme(&s, 64),
                Err(Reject::DegenerateProduct { index: 49 })
            );
        }
    }

    #[test]
    fn empty_scheme_rejected() {
        assert_eq!(verify_scheme(&[], 46), Err(Reject::EmptyScheme));
    }

    #[test]
    fn submission_codec_round_trips() {
        let s = strassen_4x4_rank49();
        let bytes = encode_scheme(&s);
        assert_eq!(bytes.len(), 49 * 6);
        assert_eq!(decode_scheme(&bytes).unwrap(), s);
    }

    #[test]
    fn market_params_codec() {
        let p = encode_market_params(46);
        assert_eq!(p[..28], [0u8; 28]);
        assert_eq!(&p[28..], &[0, 0, 0, 46]);
        assert_eq!(decode_market_params(&p), Ok(46));
        // Wrong length and dirty high bytes are malformed.
        assert_eq!(
            decode_market_params(&p[..31]),
            Err(Reject::MalformedMarketParams)
        );
        let mut dirty = p;
        dirty[0] = 1;
        assert_eq!(
            decode_market_params(&dirty),
            Err(Reject::MalformedMarketParams)
        );
    }

    #[test]
    fn check_end_to_end_bytes() {
        let s49 = encode_scheme(&strassen_4x4_rank49());
        let s64 = encode_scheme(&naive_4x4_rank64());
        assert!(check(&encode_market_params(49), &s49));
        assert!(check(&encode_market_params(64), &s64));
        // Flagship bound rejects both known schemes.
        assert!(!check(&encode_market_params(46), &s49));
        assert!(!check(&encode_market_params(46), &s64));
        // Malformed payloads are verdict-false, not panics.
        assert!(!check(&encode_market_params(49), &s49[..s49.len() - 1]));
        assert!(!check(&encode_market_params(49), &[]));
        assert!(!check(&[0u8; 31], &s49));
    }

    #[test]
    fn evaluate_builds_canonical_journal() {
        let image_id = [7u8; 32];
        let params = encode_market_params(49);
        let sub = encode_scheme(&strassen_4x4_rank49());
        let j = evaluate(image_id, &params, &sub);
        assert!(j.verdict);
        assert_eq!(j.image_id, image_id);
        assert_eq!(j.market_params_hash, journal::sha256(&params));
        assert_eq!(j.submission_hash, journal::sha256(&sub));
        assert_eq!(j.encode().len(), journal::ENCODED_LEN);
    }
}
