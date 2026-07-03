//! Adversarial review tests (toon-meta#122 review steps 3–4): attacks on
//! the flagship predicate that the unit suite does not already pin down.
//!
//! The money-critical property: `check` returning `true` MUST imply the
//! submitted triples are a genuine rank-≤bound bilinear scheme for 4×4
//! matmul over GF(2). The tests here prove the symbolic identity check is
//! COMPLETE (all 16 outputs × all 256 monomial coefficients are compared —
//! exhaustive single-coefficient error injection), that cancellation-style
//! padding cannot beat the rank bound, and that arbitrary bytes never panic
//! the decoding/checking path.

use matmul::schemes::{naive_4x4_rank64, strassen_4x4_rank49};
use matmul::{
    check, decode_market_params, encode_market_params, encode_scheme, verify_scheme, Reject,
    Triple, ENTRIES,
};

fn xorshift(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

/// Independent ground truth: evaluate the scheme on concrete GF(2) matrices.
fn scheme_multiplies(triples: &[Triple], a: u16, b: u16) -> u16 {
    let dot = |mask: u16, m: u16| ((mask & m).count_ones() & 1) as u16;
    let mut c = 0u16;
    for t in triples {
        if dot(t.u, a) & dot(t.v, b) == 1 {
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

/// The identity check is equivalent to functional correctness: a bilinear
/// form over GF(2) is determined by its values on the 256 basis pairs
/// (e_j, e_k), so agreement there IS coefficient-wise equality.
fn agrees_on_all_basis_pairs(triples: &[Triple]) -> bool {
    for j in 0..ENTRIES {
        for k in 0..ENTRIES {
            let (a, b) = (1u16 << j, 1u16 << k);
            if scheme_multiplies(triples, a, b) != true_matmul(a, b) {
                return false;
            }
        }
    }
    true
}

/// COMPLETENESS SWEEP: for every output entry o (16) and every monomial
/// coefficient position (j, k) (256), inject exactly one coefficient error
/// into an otherwise-valid scheme and require rejection. This proves no
/// output entry and no coefficient position is skipped by the verifier —
/// the property a rigged market would need to violate.
#[test]
fn every_output_and_every_coefficient_position_is_checked() {
    let base = naive_4x4_rank64();
    for o in 0..ENTRIES {
        for j in 0..ENTRIES {
            for k in 0..ENTRIES {
                let mut s = base.clone();
                let (u, v) = (1u16 << j, 1u16 << k);
                let expected = if let Some(idx) = s.iter().position(|t| t.u == u && t.v == v)
                {
                    // The naive scheme already computes monomial A_j·B_k:
                    // flip output o on that product.
                    s[idx].w ^= 1 << o;
                    if s[idx].w == 0 {
                        // Flipped away its only output: caught as degenerate.
                        verify_scheme(&s, 64) == Err(Reject::DegenerateProduct { index: idx })
                    } else {
                        verify_scheme(&s, 64) == Err(Reject::NotMatmul { output: o })
                    }
                } else {
                    // Add a brand-new single-monomial product feeding output o.
                    s.push(Triple { u, v, w: 1 << o });
                    verify_scheme(&s, 65) == Err(Reject::NotMatmul { output: o })
                };
                assert!(
                    expected,
                    "single-coefficient error at output {o}, monomial ({j},{k}) \
                     was not rejected as expected: {:?}",
                    verify_scheme(&s, 65)
                );
            }
        }
    }
}

/// Randomized equivalence: on arbitrary structurally-clean schemes, the
/// symbolic verifier's verdict must equal brute-force basis evaluation.
/// (Random schemes essentially never compute matmul; mutants of valid ones
/// sometimes get repaired back — both directions are compared.)
#[test]
fn fuzz_symbolic_check_equals_basis_evaluation() {
    let mut rng = 0xa5a5_1234u32;
    let bases = [strassen_4x4_rank49(), naive_4x4_rank64()];
    for iter in 0..4_000 {
        let mut s = bases[(xorshift(&mut rng) % 2) as usize].clone();
        // Apply 1..=4 random single-bit mutations across the scheme.
        for _ in 0..(1 + xorshift(&mut rng) % 4) {
            let i = (xorshift(&mut rng) as usize) % s.len();
            let bit = 1u16 << (xorshift(&mut rng) % 16);
            match xorshift(&mut rng) % 3 {
                0 => s[i].u ^= bit,
                1 => s[i].v ^= bit,
                _ => s[i].w ^= bit,
            }
        }
        // Skip structurally-rejected mutants: those are covered elsewhere;
        // here we compare the polynomial identity itself.
        if s.iter().any(|t| t.u == 0 || t.v == 0 || t.w == 0) {
            continue;
        }
        let mut has_dup = false;
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                if s[i].u == s[j].u && s[i].v == s[j].v {
                    has_dup = true;
                }
            }
        }
        if has_dup {
            continue;
        }
        let symbolic = verify_scheme(&s, s.len() as u32).is_ok();
        let brute = agrees_on_all_basis_pairs(&s);
        assert_eq!(symbolic, brute, "iter {iter}: symbolic vs basis drift on {s:?}");
    }
}

/// Cancellation padding attack, near-duplicate variant: split one product
/// (u,v,w) into (u, v⊕d, w) + (u, d, w) — same bilinear map, passes the
/// exact-duplicate check. It MUST still count as two products toward the
/// rank bound (r is an upper bound on true rank either way, so PASS stays
/// sound), and MUST NOT slip under the original bound.
#[test]
fn split_product_padding_counts_toward_rank() {
    let base = strassen_4x4_rank49();
    let Triple { u, v, w } = base[0];
    // Find a mask d so neither (u, v^d) nor (u, d) collides with any
    // existing (u', v') pair, and the two halves differ from each other.
    let mut padded = None;
    for d in 1u16..=0xffff {
        if d == v || d == v ^ d {
            continue;
        }
        let clash = |x: u16| base.iter().any(|t| t.u == u && t.v == x);
        if clash(v ^ d) || clash(d) {
            continue;
        }
        let mut s: Vec<Triple> = base[1..].to_vec();
        s.push(Triple { u, v: v ^ d, w });
        s.push(Triple { u, v: d, w });
        padded = Some(s);
        break;
    }
    let padded = padded.expect("a non-colliding split mask must exist");
    assert_eq!(padded.len(), 50);
    // Same bilinear map: identity holds, no duplicate rejection...
    assert_eq!(verify_scheme(&padded, 50), Ok(()));
    assert!(agrees_on_all_basis_pairs(&padded));
    // ...but the padding is charged to r: it cannot pass the tighter bound.
    assert_eq!(
        verify_scheme(&padded, 49),
        Err(Reject::RankExceeded { rank: 50, bound: 49 })
    );
}

/// Raw-bytes fuzz of the full guest path: arbitrary market params and
/// submissions must never panic, and truncated/oversized/garbage encodings
/// must be verdict-false.
#[test]
fn fuzz_check_raw_bytes_never_panics() {
    let mut rng = 0x0dd_b1a5u32;
    for _ in 0..20_000 {
        let plen = (xorshift(&mut rng) % 40) as usize;
        let params: Vec<u8> = (0..plen).map(|_| (xorshift(&mut rng) & 0xff) as u8).collect();
        let slen = (xorshift(&mut rng) % 64) as usize;
        let sub: Vec<u8> = (0..slen).map(|_| (xorshift(&mut rng) & 0xff) as u8).collect();
        let _ = check(&params, &sub); // must not panic
        if plen != 32 {
            assert!(!check(&params, &sub), "non-32-byte params must be false");
        }
    }
}

/// Market params must be EXACTLY abi.encode(uint256): trailing garbage
/// after a valid 32-byte bound is malformed, as is a 64-byte double-word.
#[test]
fn market_params_trailing_garbage_rejected() {
    let good = encode_market_params(46);
    assert_eq!(decode_market_params(&good), Ok(46));
    let mut trailing = good.to_vec();
    trailing.push(0x00);
    assert_eq!(decode_market_params(&trailing), Err(Reject::MalformedMarketParams));
    let mut two_words = good.to_vec();
    two_words.extend_from_slice(&[0u8; 32]);
    assert_eq!(decode_market_params(&two_words), Err(Reject::MalformedMarketParams));
    // And through the full check path with a valid scheme attached.
    let sub = encode_scheme(&strassen_4x4_rank49());
    assert!(check(&encode_market_params(49), &sub));
    let mut params49_trailing = encode_market_params(49).to_vec();
    params49_trailing.push(0);
    assert!(!check(&params49_trailing, &sub));
}
