//! Small 3-SAT floor predicate (toon-protocol/toon-meta#122, predicate 2).
//!
//! Proposition: "A satisfying assignment for pinned 3-SAT instance I is
//! published by date D."
//!
//! The pinned instance is a content-addressed input (part of the market
//! params); the claimed assignment is the submission. The check evaluates
//! every clause of I under the assignment; verdict is `true` iff the
//! instance and assignment are both well-formed AND every clause is
//! satisfied.
//!
//! An instance with ZERO clauses is well-formed and vacuously satisfied by
//! any correctly-sized assignment (verdict `true`). This is deliberate: the
//! instance is pinned and adversarially reviewed at market creation, so a
//! trivially-true market is the creator's visible choice, never a submitter
//! exploit. Reviewers of a pinned instance must reject empty instances.
//!
//! Failure-mode spec (normative): malformed input — wrong assignment
//! length, non-boolean assignment byte, a clause that is not exactly three
//! literals, a zero or out-of-range literal, instance-size ceilings
//! exceeded, or a byte blob that does not decode — MUST yield verdict
//! `false` (a clean rejection), never a panic. A panicking guest cannot
//! produce a PASS proof either way, but the failure mode is spec'd so the
//! natural-language proposition and the code agree exactly.

/// Canonical journal envelope (toon-meta#121), shared across predicates.
pub use journal;

use std::fmt;

/// Ceilings on instance size, pinned as part of the predicate so proving
/// cost is bounded. An instance exceeding these is malformed.
pub const MAX_VARS: u32 = 1024;
pub const MAX_CLAUSES: usize = 4096;

/// A 3-SAT instance in DIMACS-style signed-literal form.
///
/// Literals are 1-indexed: literal `+k` means variable `k` is true,
/// literal `-k` means variable `k` is false. `0` is not a valid literal.
/// A well-formed clause has exactly three literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub num_vars: u32,
    pub clauses: Vec<Vec<i32>>,
}

/// Why an input was rejected as malformed. Every variant maps to verdict
/// `false` in the guest; none may panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Malformed {
    /// Instance declares zero variables or more than [`MAX_VARS`].
    VarCountOutOfRange(u32),
    /// Instance has more than [`MAX_CLAUSES`] clauses.
    TooManyClauses(usize),
    /// A clause does not have exactly three literals (includes the empty
    /// clause, which in pure SAT semantics would be trivially unsatisfiable
    /// — here it is rejected as malformed 3-SAT instead).
    ClauseNotThreeLiterals { clause_index: usize, len: usize },
    /// A literal is zero or references a variable outside `1..=num_vars`.
    LiteralOutOfRange { clause_index: usize, literal: i32 },
    /// Assignment length does not equal `num_vars` (truncated or padded).
    AssignmentLengthMismatch { expected: u32, actual: usize },
    /// Byte blob does not decode as the canonical encoding.
    BadEncoding(&'static str),
}

impl fmt::Display for Malformed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Malformed::VarCountOutOfRange(n) => {
                write!(f, "variable count {n} outside 1..={MAX_VARS}")
            }
            Malformed::TooManyClauses(n) => {
                write!(f, "clause count {n} exceeds {MAX_CLAUSES}")
            }
            Malformed::ClauseNotThreeLiterals { clause_index, len } => {
                write!(f, "clause {clause_index} has {len} literals, expected exactly 3")
            }
            Malformed::LiteralOutOfRange { clause_index, literal } => {
                write!(f, "clause {clause_index} literal {literal} is zero or out of range")
            }
            Malformed::AssignmentLengthMismatch { expected, actual } => {
                write!(f, "assignment has {actual} values, instance has {expected} variables")
            }
            Malformed::BadEncoding(what) => write!(f, "bad encoding: {what}"),
        }
    }
}

/// Validate that `instance` is well-formed 3-SAT within the pinned ceilings.
pub fn validate_instance(instance: &Instance) -> Result<(), Malformed> {
    if instance.num_vars == 0 || instance.num_vars > MAX_VARS {
        return Err(Malformed::VarCountOutOfRange(instance.num_vars));
    }
    if instance.clauses.len() > MAX_CLAUSES {
        return Err(Malformed::TooManyClauses(instance.clauses.len()));
    }
    for (clause_index, clause) in instance.clauses.iter().enumerate() {
        if clause.len() != 3 {
            return Err(Malformed::ClauseNotThreeLiterals { clause_index, len: clause.len() });
        }
        for &literal in clause {
            // checked_abs also rejects i32::MIN without overflow.
            let var = literal.checked_abs().unwrap_or(0) as u32;
            if var == 0 || var > instance.num_vars {
                return Err(Malformed::LiteralOutOfRange { clause_index, literal });
            }
        }
    }
    Ok(())
}

/// Evaluate the claimed assignment against the pinned instance.
///
/// Returns `Ok(true)` iff every clause is satisfied, `Ok(false)` if at least
/// one clause is falsified, and `Err(Malformed)` on any malformed input.
pub fn check(instance: &Instance, assignment: &[bool]) -> Result<bool, Malformed> {
    validate_instance(instance)?;
    if assignment.len() != instance.num_vars as usize {
        return Err(Malformed::AssignmentLengthMismatch {
            expected: instance.num_vars,
            actual: assignment.len(),
        });
    }
    for clause in &instance.clauses {
        let satisfied = clause.iter().any(|&literal| {
            let var = literal.unsigned_abs() as usize; // validated: 1..=num_vars
            let value = assignment[var - 1];
            if literal > 0 {
                value
            } else {
                !value
            }
        });
        if !satisfied {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The verdict the guest commits to the journal: `true` iff the input is
/// well-formed AND satisfies every clause. Malformed input folds to `false`
/// — this function never panics on any input.
pub fn verdict(instance: &Instance, assignment: &[bool]) -> bool {
    check(instance, assignment).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Canonical byte encoding (content-addressed inputs arrive as byte blobs).
// ---------------------------------------------------------------------------

/// Decode the canonical instance encoding:
/// `num_vars: u32 LE | num_clauses: u32 LE | num_clauses × (3 × i32 LE)`.
///
/// The encoding fixes exactly three literals per clause; semantic validity
/// (literal ranges, ceilings) is still checked by [`validate_instance`].
pub fn decode_instance(bytes: &[u8]) -> Result<Instance, Malformed> {
    let header = 8usize;
    if bytes.len() < header {
        return Err(Malformed::BadEncoding("instance blob shorter than 8-byte header"));
    }
    let num_vars = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let num_clauses = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let expected_len = header
        .checked_add(num_clauses.checked_mul(12).ok_or(Malformed::BadEncoding(
            "clause count overflows length computation",
        ))?)
        .ok_or(Malformed::BadEncoding("clause count overflows length computation"))?;
    if bytes.len() != expected_len {
        return Err(Malformed::BadEncoding("instance blob length does not match clause count"));
    }
    let mut clauses = Vec::with_capacity(num_clauses.min(MAX_CLAUSES));
    if num_clauses > MAX_CLAUSES {
        return Err(Malformed::TooManyClauses(num_clauses));
    }
    for c in 0..num_clauses {
        let base = header + c * 12;
        let clause = (0..3)
            .map(|i| {
                let off = base + i * 4;
                i32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
            })
            .collect();
        clauses.push(clause);
    }
    let instance = Instance { num_vars, clauses };
    validate_instance(&instance)?;
    Ok(instance)
}

/// Decode the canonical assignment encoding: one byte per variable, `0x00`
/// for false, `0x01` for true. Any other byte value is malformed. Length is
/// checked against the instance inside [`check`].
pub fn decode_assignment(bytes: &[u8]) -> Result<Vec<bool>, Malformed> {
    bytes
        .iter()
        .map(|&b| match b {
            0x00 => Ok(false),
            0x01 => Ok(true),
            _ => Err(Malformed::BadEncoding("assignment byte is not 0x00 or 0x01")),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Pinned launch fixture.
// ---------------------------------------------------------------------------

/// The small hand-picked 3-SAT instance pinned for the launch market. Its
/// canonical encoding is what gets content-addressed into the input
/// manifest as `instance`.
pub mod fixture {
    use super::Instance;

    /// 4 variables, 5 clauses. Satisfied by `x1=T, x2=F, x3=T, x4=T`.
    pub fn pinned_instance() -> Instance {
        Instance {
            num_vars: 4,
            clauses: vec![
                vec![1, 2, -3],
                vec![-1, 3, 4],
                vec![-2, -3, 4],
                vec![1, -2, -4],
                vec![-1, 2, 3],
            ],
        }
    }

    /// A known satisfying assignment for [`pinned_instance`].
    pub fn satisfying_assignment() -> Vec<bool> {
        vec![true, false, true, true]
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{pinned_instance, satisfying_assignment};
    use super::*;

    #[test]
    fn positive_satisfying_assignment() {
        let instance = pinned_instance();
        assert_eq!(check(&instance, &satisfying_assignment()), Ok(true));
        assert!(verdict(&instance, &satisfying_assignment()));
    }

    #[test]
    fn negative_assignment_misses_a_clause() {
        // x1=T, x2=T, x3=T, x4=F falsifies clause 2: (-2, -3, 4).
        let instance = pinned_instance();
        let assignment = vec![true, true, true, false];
        assert_eq!(check(&instance, &assignment), Ok(false));
        assert!(!verdict(&instance, &assignment));
    }

    #[test]
    fn negative_wrong_variable_count() {
        // Assignment for 5 variables against a 4-variable instance.
        let instance = pinned_instance();
        let assignment = vec![true, false, true, true, true];
        assert_eq!(
            check(&instance, &assignment),
            Err(Malformed::AssignmentLengthMismatch { expected: 4, actual: 5 })
        );
        assert!(!verdict(&instance, &assignment));
    }

    #[test]
    fn negative_truncated_assignment() {
        let instance = pinned_instance();
        let assignment = vec![true, false, true];
        assert_eq!(
            check(&instance, &assignment),
            Err(Malformed::AssignmentLengthMismatch { expected: 4, actual: 3 })
        );
        assert!(!verdict(&instance, &assignment));
    }

    #[test]
    fn negative_empty_clause_is_malformed_not_panic() {
        let instance = Instance {
            num_vars: 4,
            clauses: vec![vec![1, 2, -3], vec![]],
        };
        assert_eq!(
            check(&instance, &satisfying_assignment()),
            Err(Malformed::ClauseNotThreeLiterals { clause_index: 1, len: 0 })
        );
        assert!(!verdict(&instance, &satisfying_assignment()));
    }

    #[test]
    fn negative_out_of_range_literal_is_malformed_not_panic() {
        // Variable 5 does not exist in a 4-variable instance.
        let instance = Instance {
            num_vars: 4,
            clauses: vec![vec![1, 2, 5]],
        };
        let assignment = satisfying_assignment();
        assert_eq!(
            check(&instance, &assignment),
            Err(Malformed::LiteralOutOfRange { clause_index: 0, literal: 5 })
        );
        assert!(!verdict(&instance, &assignment));
    }

    #[test]
    fn negative_zero_literal_is_malformed() {
        let instance = Instance {
            num_vars: 4,
            clauses: vec![vec![1, 0, -3]],
        };
        assert!(!verdict(&instance, &satisfying_assignment()));
    }

    #[test]
    fn negative_i32_min_literal_does_not_panic() {
        let instance = Instance {
            num_vars: 4,
            clauses: vec![vec![1, i32::MIN, -3]],
        };
        assert!(!verdict(&instance, &satisfying_assignment()));
    }

    #[test]
    fn negative_zero_variables_is_malformed() {
        let instance = Instance { num_vars: 0, clauses: vec![] };
        assert_eq!(check(&instance, &[]), Err(Malformed::VarCountOutOfRange(0)));
    }

    #[test]
    fn negative_ceilings_enforced() {
        let too_many_vars = Instance { num_vars: MAX_VARS + 1, clauses: vec![] };
        assert!(!verdict(&too_many_vars, &vec![false; (MAX_VARS + 1) as usize]));

        let too_many_clauses = Instance {
            num_vars: 4,
            clauses: vec![vec![1, 2, 3]; MAX_CLAUSES + 1],
        };
        assert!(!verdict(&too_many_clauses, &satisfying_assignment()));
    }

    // -- canonical encoding round-trips ------------------------------------

    fn encode_instance(instance: &Instance) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&instance.num_vars.to_le_bytes());
        out.extend_from_slice(&(instance.clauses.len() as u32).to_le_bytes());
        for clause in &instance.clauses {
            for &literal in clause {
                out.extend_from_slice(&literal.to_le_bytes());
            }
        }
        out
    }

    #[test]
    fn decode_pinned_instance_round_trip() {
        let instance = pinned_instance();
        let bytes = encode_instance(&instance);
        assert_eq!(decode_instance(&bytes), Ok(instance));
    }

    #[test]
    fn decode_rejects_truncated_blob() {
        let mut bytes = encode_instance(&pinned_instance());
        bytes.pop();
        assert!(matches!(decode_instance(&bytes), Err(Malformed::BadEncoding(_))));
        assert!(matches!(decode_instance(&[1, 2, 3]), Err(Malformed::BadEncoding(_))));
        assert!(matches!(decode_instance(&[]), Err(Malformed::BadEncoding(_))));
    }

    #[test]
    fn decode_rejects_out_of_range_literal_in_blob() {
        let instance = Instance { num_vars: 4, clauses: vec![vec![1, 2, 99]] };
        let bytes = encode_instance(&instance);
        assert_eq!(
            decode_instance(&bytes),
            Err(Malformed::LiteralOutOfRange { clause_index: 0, literal: 99 })
        );
    }

    #[test]
    fn decode_assignment_rejects_non_boolean_byte() {
        assert_eq!(decode_assignment(&[0, 1, 1, 0]), Ok(vec![false, true, true, false]));
        assert!(matches!(decode_assignment(&[0, 1, 2, 0]), Err(Malformed::BadEncoding(_))));
    }

    #[test]
    fn journal_struct_matches_envelope_spec() {
        // Shape check against toon-meta#121: four fields, three 32-byte
        // hashes plus a bool verdict.
        let j = journal::Journal {
            image_id: [0u8; 32],
            market_params_hash: [1u8; 32],
            submission_hash: [2u8; 32],
            verdict: true,
        };
        assert!(j.verdict);
    }
}
