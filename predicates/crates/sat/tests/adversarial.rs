//! Adversarial review tests (toon-meta#122 review step 4): fuzz the byte
//! decoders for panics and cross-validate clause evaluation against an
//! independent reference evaluator, so a sign/indexing slip in `check`
//! cannot survive unnoticed.

use sat::{check, decode_assignment, decode_instance, verdict, Instance, MAX_VARS};

fn xorshift(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

/// Independent reference evaluator, written against the DIMACS convention
/// directly (literal +k true iff variable k assigned true; -k iff false;
/// variables 1-indexed). Any drift between this and `check` on well-formed
/// input is a bug in one of them.
fn reference_eval(instance: &Instance, assignment: &[bool]) -> bool {
    instance.clauses.iter().all(|clause| {
        clause.iter().any(|&lit| {
            let idx = (lit.unsigned_abs() - 1) as usize;
            if lit > 0 { assignment[idx] } else { !assignment[idx] }
        })
    })
}

#[test]
fn fuzz_check_matches_reference_evaluator() {
    let mut rng = 0xc0ff_ee11u32;
    for _ in 0..10_000 {
        let num_vars = 1 + xorshift(&mut rng) % 12;
        let num_clauses = (xorshift(&mut rng) % 20) as usize;
        let clauses: Vec<Vec<i32>> = (0..num_clauses)
            .map(|_| {
                (0..3)
                    .map(|_| {
                        let var = (1 + xorshift(&mut rng) % num_vars) as i32;
                        if xorshift(&mut rng) & 1 == 1 { var } else { -var }
                    })
                    .collect()
            })
            .collect();
        let instance = Instance { num_vars, clauses };
        let assignment: Vec<bool> =
            (0..num_vars).map(|_| xorshift(&mut rng) & 1 == 1).collect();
        assert_eq!(
            check(&instance, &assignment),
            Ok(reference_eval(&instance, &assignment)),
            "drift from reference evaluator: {instance:?} {assignment:?}"
        );
    }
}

/// Random byte blobs through both decoders: must never panic, and whatever
/// decodes must re-validate cleanly (decoder output is always well-formed).
#[test]
fn fuzz_decoders_never_panic() {
    let mut rng = 0xdead_4471u32;
    for _ in 0..20_000 {
        let len = (xorshift(&mut rng) % 96) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| (xorshift(&mut rng) & 0xff) as u8).collect();
        if let Ok(instance) = decode_instance(&bytes) {
            // Anything the decoder accepts must be well-formed 3-SAT.
            assert!(sat::validate_instance(&instance).is_ok());
            assert!(instance.num_vars >= 1 && instance.num_vars <= MAX_VARS);
        }
        let _ = decode_assignment(&bytes);
    }
}

/// Structured fuzz: take a valid encoding and mutate one byte / truncate /
/// extend. Must never panic; truncation and extension must always be
/// rejected (strict length).
#[test]
fn fuzz_mutated_valid_encoding() {
    let instance = sat::fixture::pinned_instance();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&instance.num_vars.to_le_bytes());
    bytes.extend_from_slice(&(instance.clauses.len() as u32).to_le_bytes());
    for clause in &instance.clauses {
        for &lit in clause {
            bytes.extend_from_slice(&lit.to_le_bytes());
        }
    }
    assert_eq!(decode_instance(&bytes), Ok(instance));

    let mut rng = 0x5eed_5eedu32;
    for _ in 0..5_000 {
        let mut m = bytes.clone();
        let pos = (xorshift(&mut rng) as usize) % m.len();
        m[pos] ^= (xorshift(&mut rng) & 0xff) as u8;
        let _ = decode_instance(&m); // no panic; may be Ok or Err
    }
    for cut in 0..bytes.len() {
        assert!(decode_instance(&bytes[..cut]).is_err(), "truncation at {cut} accepted");
    }
    let mut extended = bytes.clone();
    extended.push(0);
    assert!(decode_instance(&extended).is_err(), "trailing garbage accepted");
}

/// Pin the vacuous-truth semantics: a well-formed instance with ZERO
/// clauses is satisfied by any correctly-sized assignment. This is
/// intentional (the instance is pinned and reviewed at market creation, so
/// a trivially-true market is the creator's visible choice, not a submitter
/// exploit) — but it must stay a conscious, documented behavior.
#[test]
fn zero_clause_instance_is_vacuously_satisfied() {
    let instance = Instance { num_vars: 2, clauses: vec![] };
    assert_eq!(check(&instance, &[true, false]), Ok(true));
    assert!(verdict(&instance, &[true, false]));
    // ... but the assignment length is still enforced.
    assert!(!verdict(&instance, &[true]));
    assert!(!verdict(&instance, &[]));
}
