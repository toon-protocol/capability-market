//! Differential-testing divergence predicate (toon-protocol/toon-meta#122,
//! predicate 3).
//!
//! Proposition: "An input on which pinned programs P1 and P2 produce
//! different outputs is published by date D."
//!
//! Arbitrary native programs cannot run inside a zkVM guest, so P1 and P2
//! are pinned as bytecode blobs for a tiny deterministic stack-machine
//! interpreter defined in this crate. The interpreter has a fixed
//! instruction set and a hard step ceiling ([`MAX_STEPS`]) that is part of
//! the pinned params, so proving cost is bounded by construction.
//!
//! Check: run both programs on the claimed input; verdict `true` iff BOTH
//! terminate cleanly (a `HALT` within the step budget, no trap) AND their
//! outputs differ. Spec'd explicitly: non-termination (step budget
//! exceeded), any trap (stack underflow/overflow, unknown opcode, truncated
//! immediate, out-of-bounds jump, input index out of range, division by
//! zero, running off the end of the program, halting with an empty stack),
//! or malformed input encoding all yield verdict `false` — never a panic.

/// Canonical journal envelope (toon-meta#121), shared across predicates.
pub use journal;

use std::fmt;

/// Hard step ceiling per program run. Part of the pinned predicate params:
/// it bounds guest cycles and therefore proving cost. Exceeding it is
/// non-termination for the purposes of the proposition → verdict `false`.
pub const MAX_STEPS: u64 = 100_000;

/// Maximum operand-stack depth. Pushing beyond it traps.
pub const MAX_STACK: usize = 256;

/// Maximum program size in bytes. Larger blobs are malformed.
pub const MAX_PROGRAM_BYTES: usize = 4096;

/// Maximum number of 64-bit input words. Longer inputs are malformed.
pub const MAX_INPUT_WORDS: usize = 64;

/// Instruction set of the pinned stack machine. All arithmetic is on `u64`
/// with wrapping semantics; every instruction is deterministic.
pub mod op {
    /// `PUSH imm` — push the following 8-byte little-endian u64. `0x01 imm[8]`
    pub const PUSH: u8 = 0x01;
    /// `POP` — discard the top of stack.
    pub const POP: u8 = 0x02;
    /// `ADD` — pop b, pop a, push `a.wrapping_add(b)`.
    pub const ADD: u8 = 0x03;
    /// `SUB` — pop b, pop a, push `a.wrapping_sub(b)`.
    pub const SUB: u8 = 0x04;
    /// `MUL` — pop b, pop a, push `a.wrapping_mul(b)`.
    pub const MUL: u8 = 0x05;
    /// `DIV` — pop b, pop a, push `a / b`; traps if `b == 0`.
    pub const DIV: u8 = 0x06;
    /// `DUP` — duplicate the top of stack.
    pub const DUP: u8 = 0x07;
    /// `SWAP` — swap the top two stack values.
    pub const SWAP: u8 = 0x08;
    /// `INPUT idx` — push input word `idx` (one-byte index). `0x09 idx[1]`
    pub const INPUT: u8 = 0x09;
    /// `JMP addr` — unconditional jump to byte offset `addr`. `0x0A addr[2 LE]`
    pub const JMP: u8 = 0x0a;
    /// `JZ addr` — pop v; jump to `addr` if `v == 0`. `0x0B addr[2 LE]`
    pub const JZ: u8 = 0x0b;
    /// `HALT` — terminate; the output is the top of stack (popped).
    pub const HALT: u8 = 0x0c;
}

/// Why a program run failed to produce an output. Any variant → verdict
/// `false` for the divergence check; none may panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecError {
    /// Program blob exceeds [`MAX_PROGRAM_BYTES`] or is empty.
    ProgramSizeOutOfRange(usize),
    /// The step ceiling [`MAX_STEPS`] was reached before `HALT`.
    StepBudgetExceeded,
    /// Unknown opcode byte at the given offset (malformed bytecode).
    UnknownOpcode { pc: usize, opcode: u8 },
    /// An instruction's immediate runs past the end of the program.
    TruncatedImmediate { pc: usize },
    /// Execution ran off the end of the program without `HALT`.
    PcOutOfBounds,
    /// A jump targeted a byte offset outside the program.
    JumpOutOfBounds { pc: usize, target: usize },
    /// A pop/DUP/SWAP/HALT found too few values on the stack.
    StackUnderflow { pc: usize },
    /// A push exceeded [`MAX_STACK`].
    StackOverflow { pc: usize },
    /// `INPUT idx` referenced a word beyond the input length.
    InputIndexOutOfRange { pc: usize, index: usize },
    /// `DIV` with a zero divisor.
    DivisionByZero { pc: usize },
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Why the claimed input blob was rejected before execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Malformed {
    /// Input blob length is not a multiple of 8.
    InputNotWordAligned(usize),
    /// Input has more than [`MAX_INPUT_WORDS`] words.
    TooManyInputWords(usize),
}

/// Decode the claimed input: a sequence of 8-byte little-endian u64 words,
/// at most [`MAX_INPUT_WORDS`] of them.
pub fn decode_input(bytes: &[u8]) -> Result<Vec<u64>, Malformed> {
    if !bytes.len().is_multiple_of(8) {
        return Err(Malformed::InputNotWordAligned(bytes.len()));
    }
    let words = bytes.len() / 8;
    if words > MAX_INPUT_WORDS {
        return Err(Malformed::TooManyInputWords(words));
    }
    Ok(bytes
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
        .collect())
}

/// Run `program` on `input` under the pinned machine semantics. Returns the
/// single `u64` output on clean `HALT`, or the reason the run failed. Never
/// panics on any byte sequence.
pub fn run(program: &[u8], input: &[u64]) -> Result<u64, ExecError> {
    if program.is_empty() || program.len() > MAX_PROGRAM_BYTES {
        return Err(ExecError::ProgramSizeOutOfRange(program.len()));
    }
    let mut stack: Vec<u64> = Vec::with_capacity(MAX_STACK);
    let mut pc: usize = 0;
    let mut steps: u64 = 0;

    macro_rules! pop {
        () => {
            stack.pop().ok_or(ExecError::StackUnderflow { pc })?
        };
    }

    loop {
        if steps >= MAX_STEPS {
            return Err(ExecError::StepBudgetExceeded);
        }
        steps += 1;
        if pc >= program.len() {
            return Err(ExecError::PcOutOfBounds);
        }
        let opcode = program[pc];
        match opcode {
            op::PUSH => {
                let end = pc + 9;
                if end > program.len() {
                    return Err(ExecError::TruncatedImmediate { pc });
                }
                if stack.len() >= MAX_STACK {
                    return Err(ExecError::StackOverflow { pc });
                }
                let imm = u64::from_le_bytes(program[pc + 1..end].try_into().unwrap());
                stack.push(imm);
                pc = end;
            }
            op::POP => {
                pop!();
                pc += 1;
            }
            op::ADD | op::SUB | op::MUL | op::DIV => {
                let b = pop!();
                let a = pop!();
                let r = match opcode {
                    op::ADD => a.wrapping_add(b),
                    op::SUB => a.wrapping_sub(b),
                    op::MUL => a.wrapping_mul(b),
                    _ => {
                        if b == 0 {
                            return Err(ExecError::DivisionByZero { pc });
                        }
                        a / b
                    }
                };
                stack.push(r);
                pc += 1;
            }
            op::DUP => {
                let v = *stack.last().ok_or(ExecError::StackUnderflow { pc })?;
                if stack.len() >= MAX_STACK {
                    return Err(ExecError::StackOverflow { pc });
                }
                stack.push(v);
                pc += 1;
            }
            op::SWAP => {
                let n = stack.len();
                if n < 2 {
                    return Err(ExecError::StackUnderflow { pc });
                }
                stack.swap(n - 1, n - 2);
                pc += 1;
            }
            op::INPUT => {
                let end = pc + 2;
                if end > program.len() {
                    return Err(ExecError::TruncatedImmediate { pc });
                }
                let index = program[pc + 1] as usize;
                if index >= input.len() {
                    return Err(ExecError::InputIndexOutOfRange { pc, index });
                }
                if stack.len() >= MAX_STACK {
                    return Err(ExecError::StackOverflow { pc });
                }
                stack.push(input[index]);
                pc = end;
            }
            op::JMP | op::JZ => {
                let end = pc + 3;
                if end > program.len() {
                    return Err(ExecError::TruncatedImmediate { pc });
                }
                let target =
                    u16::from_le_bytes(program[pc + 1..end].try_into().unwrap()) as usize;
                let jump = if opcode == op::JMP { true } else { pop!() == 0 };
                if jump {
                    if target >= program.len() {
                        return Err(ExecError::JumpOutOfBounds { pc, target });
                    }
                    pc = target;
                } else {
                    pc = end;
                }
            }
            op::HALT => {
                return Ok(pop!());
            }
            other => {
                return Err(ExecError::UnknownOpcode { pc, opcode: other });
            }
        }
    }
}

/// The divergence check the guest commits as its verdict: `true` iff BOTH
/// pinned programs terminate cleanly on the claimed input within the step
/// budget AND their outputs differ. Any failed run — non-termination, trap,
/// malformed bytecode — yields `false`. Never panics.
pub fn check(p1: &[u8], p2: &[u8], input: &[u64]) -> bool {
    match (run(p1, input), run(p2, input)) {
        (Ok(out1), Ok(out2)) => out1 != out2,
        _ => false,
    }
}

/// Convenience wrapper taking the raw submission bytes: decodes the claimed
/// input blob, then runs [`check`]. Malformed input encoding → `false`.
pub fn verdict(p1: &[u8], p2: &[u8], input_bytes: &[u8]) -> bool {
    match decode_input(input_bytes) {
        Ok(input) => check(p1, p2, &input),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Input-manifest layer (toon-meta#121, resolves capability-market#4)
// ---------------------------------------------------------------------------
//
// `marketParamsHash = sha256(canonical manifest bytes)`. The guest reads
// `(image_id, manifest_bytes, submission)`: it extracts the two pinned program
// blobs (and the declared step budget) from the manifest by name and commits
// `sha256(manifest_bytes)` as `market_params_hash`. Both programs are embedded
// as literal VALUE entries so the guest needs no side-channel input.

/// Manifest entry name for the first pinned program blob P1.
pub const MANIFEST_PROGRAM_1: &str = "program_1";
/// Manifest entry name for the second pinned program blob P2.
pub const MANIFEST_PROGRAM_2: &str = "program_2";
/// Manifest entry name for the pinned step budget, declared as a `u64` LE
/// literal for audit. The interpreter enforces the compiled-in [`MAX_STEPS`]
/// (baked into the guest image); this entry MUST equal it, so the pinned
/// proving-cost ceiling is visible in the market's frozen parameters.
pub const MANIFEST_STEP_BUDGET: &str = "step_budget";
/// Manifest entry name for the deadline literal (audit only; time-independent).
pub const MANIFEST_FROZEN_CLOCK: &str = "frozen_clock";
/// Manifest entry name for the late-bound submission slot (the claimed input).
pub const MANIFEST_SUBMISSION: &str = "submission";

/// Build the canonical difftest input manifest: the two program blobs as
/// `program_1` / `program_2` VALUEs, the step budget and deadline as literal
/// VALUEs, and the late-bound `submission` SLOT. `sha256` of the bytes is the
/// market's `marketParamsHash`.
pub fn encode_manifest(p1: &[u8], p2: &[u8], frozen_clock: u64) -> Vec<u8> {
    manifest::encode(&[
        manifest::Entry::value(MANIFEST_PROGRAM_1, p1.to_vec()),
        manifest::Entry::value(MANIFEST_PROGRAM_2, p2.to_vec()),
        manifest::Entry::value(MANIFEST_STEP_BUDGET, MAX_STEPS.to_le_bytes().to_vec()),
        manifest::Entry::value(MANIFEST_FROZEN_CLOCK, frozen_clock.to_le_bytes().to_vec()),
        manifest::Entry::slot(MANIFEST_SUBMISSION),
    ])
    .expect("static difftest manifest is always canonical")
}

/// Manifest-driven check: parse the manifest, extract the two programs (and
/// verify the declared `step_budget` matches the compiled-in [`MAX_STEPS`]),
/// then run the unchanged divergence [`verdict`] over the submission. Any
/// parse/decode failure — or a step-budget declaration that does not match the
/// pinned interpreter ceiling — is `false`, never a panic.
pub fn check_manifest(manifest_bytes: &[u8], submission: &[u8]) -> bool {
    let Ok(m) = manifest::parse(manifest_bytes) else {
        return false;
    };
    let (Some(p1), Some(p2)) = (m.value(MANIFEST_PROGRAM_1), m.value(MANIFEST_PROGRAM_2)) else {
        return false;
    };
    // The declared budget must match what the interpreter actually enforces,
    // so the manifest cannot advertise a ceiling the guest does not honour.
    match m.value(MANIFEST_STEP_BUDGET) {
        Some(b) if b == MAX_STEPS.to_le_bytes() => {}
        _ => return false,
    }
    verdict(p1, p2, submission)
}

/// Full guest computation, host-callable for tests: judge the submission
/// against the manifest and assemble the canonical journal exactly as the
/// guest commits it (`market_params_hash = sha256(manifest_bytes)`).
pub fn evaluate(image_id: [u8; 32], manifest_bytes: &[u8], submission: &[u8]) -> journal::Journal {
    let verdict = check_manifest(manifest_bytes, submission);
    journal::predicate_journal(image_id, manifest_bytes, submission, verdict)
}

/// Encode a claimed input to canonical submission bytes (little-endian `u64`
/// words; inverse of [`decode_input`]).
pub fn encode_input(words: &[u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(words.len() * 8);
    for &w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out
}

/// Tiny assembler helpers for building fixture programs in tests and docs.
pub mod asm {
    use super::op;

    pub fn push(imm: u64) -> Vec<u8> {
        let mut v = vec![op::PUSH];
        v.extend_from_slice(&imm.to_le_bytes());
        v
    }

    pub fn input(idx: u8) -> Vec<u8> {
        vec![op::INPUT, idx]
    }

    pub fn jmp(addr: u16) -> Vec<u8> {
        let mut v = vec![op::JMP];
        v.extend_from_slice(&addr.to_le_bytes());
        v
    }

    pub fn concat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }
}

/// The pinned launch fixture programs P1 and P2. Their byte blobs are what
/// get content-addressed into the input manifest.
pub mod fixture {
    use super::{asm, op};

    /// P1: the identity map on the first input word — `f(x) = x`.
    pub fn p1_identity() -> Vec<u8> {
        asm::concat(&[asm::input(0), vec![op::HALT]])
    }

    /// P2: the square of the first input word — `g(x) = x·x` (wrapping).
    ///
    /// P1 and P2 agree exactly on the fixed points of squaring (x = 0 and
    /// x = 1) and diverge everywhere else, so a divergence witness exists
    /// but not every input is one.
    pub fn p2_square() -> Vec<u8> {
        asm::concat(&[asm::input(0), vec![op::DUP, op::MUL, op::HALT]])
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{p1_identity, p2_square};
    use super::*;

    #[test]
    fn positive_divergent_input() {
        // x = 2: P1 outputs 2, P2 outputs 4 → verdict true.
        assert_eq!(run(&p1_identity(), &[2]), Ok(2));
        assert_eq!(run(&p2_square(), &[2]), Ok(4));
        assert!(check(&p1_identity(), &p2_square(), &[2]));
        assert!(verdict(&p1_identity(), &p2_square(), &2u64.to_le_bytes()));
    }

    #[test]
    fn negative_identical_programs() {
        assert!(!check(&p1_identity(), &p1_identity(), &[2]));
        assert!(!check(&p2_square(), &p2_square(), &[7]));
    }

    #[test]
    fn negative_input_where_programs_agree() {
        // 0 and 1 are the fixed points of squaring.
        assert!(!check(&p1_identity(), &p2_square(), &[0]));
        assert!(!check(&p1_identity(), &p2_square(), &[1]));
    }

    #[test]
    fn negative_step_budget_exceeded_is_not_divergence() {
        // `JMP 0` loops forever; the interpreter cuts it at MAX_STEPS.
        let looper = asm::jmp(0);
        assert_eq!(run(&looper, &[]), Err(ExecError::StepBudgetExceeded));
        // Even though the other program halts fine, verdict is false.
        assert!(!check(&looper, &p1_identity(), &[2]));
        assert!(!check(&p1_identity(), &looper, &[2]));
    }

    #[test]
    fn negative_malformed_bytecode_no_panic() {
        // Unknown opcode.
        assert!(matches!(
            run(&[0xff], &[]),
            Err(ExecError::UnknownOpcode { pc: 0, opcode: 0xff })
        ));
        // Truncated PUSH immediate.
        assert!(matches!(run(&[op::PUSH, 1, 2], &[]), Err(ExecError::TruncatedImmediate { .. })));
        // Truncated INPUT / JMP immediates.
        assert!(matches!(run(&[op::INPUT], &[]), Err(ExecError::TruncatedImmediate { .. })));
        assert!(matches!(run(&[op::JMP, 0], &[]), Err(ExecError::TruncatedImmediate { .. })));
        // Empty and oversized programs.
        assert!(matches!(run(&[], &[]), Err(ExecError::ProgramSizeOutOfRange(0))));
        assert!(matches!(
            run(&vec![op::POP; MAX_PROGRAM_BYTES + 1], &[]),
            Err(ExecError::ProgramSizeOutOfRange(_))
        ));
        // Malformed bytecode against a healthy program → verdict false.
        assert!(!check(&[0xff], &p1_identity(), &[2]));
    }

    #[test]
    fn negative_traps_yield_false_not_panic() {
        // Stack underflow: POP on empty stack.
        assert!(matches!(run(&[op::POP], &[]), Err(ExecError::StackUnderflow { .. })));
        // HALT with empty stack.
        assert!(matches!(run(&[op::HALT], &[]), Err(ExecError::StackUnderflow { .. })));
        // Running off the end without HALT.
        let no_halt = asm::push(1);
        assert!(matches!(run(&no_halt, &[]), Err(ExecError::PcOutOfBounds)));
        // Division by zero.
        let div0 = asm::concat(&[asm::push(1), asm::push(0), vec![op::DIV, op::HALT]]);
        assert!(matches!(run(&div0, &[]), Err(ExecError::DivisionByZero { .. })));
        // Input index out of range.
        assert!(matches!(
            run(&p1_identity(), &[]),
            Err(ExecError::InputIndexOutOfRange { .. })
        ));
        // Jump out of bounds.
        let bad_jmp = asm::jmp(9999);
        assert!(matches!(run(&bad_jmp, &[]), Err(ExecError::JumpOutOfBounds { .. })));
        // Stack overflow: unbounded pushes via a loop.
        let overflow = asm::concat(&[asm::push(1), asm::jmp(0)]);
        assert!(matches!(run(&overflow, &[]), Err(ExecError::StackOverflow { .. })));
        // Any trap on either side → verdict false.
        assert!(!check(&div0, &p1_identity(), &[2]));
    }

    #[test]
    fn negative_malformed_input_blob() {
        // Not 8-byte aligned.
        assert_eq!(decode_input(&[1, 2, 3]), Err(Malformed::InputNotWordAligned(3)));
        assert!(!verdict(&p1_identity(), &p2_square(), &[1, 2, 3]));
        // Too many words.
        let big = vec![0u8; (MAX_INPUT_WORDS + 1) * 8];
        assert_eq!(decode_input(&big), Err(Malformed::TooManyInputWords(MAX_INPUT_WORDS + 1)));
        assert!(!verdict(&p1_identity(), &p2_square(), &big));
    }

    #[test]
    fn conditional_and_arithmetic_ops_execute() {
        // if x == 0 { 100 } else { x - 1 }, exercising JZ/SUB/SWAP paths.
        // Layout: INPUT 0 (2B) | DUP (1B) | JZ 17 (3B) | PUSH 1 (9B) |
        //         SUB (1B) | HALT (1B) | @17: POP (1B) | PUSH 100 (9B) | HALT
        let prog = asm::concat(&[
            asm::input(0),
            vec![op::DUP],
            vec![op::JZ, 17, 0],
            asm::push(1),
            vec![op::SUB, op::HALT],
            vec![op::POP],
            asm::push(100),
            vec![op::HALT],
        ]);
        assert_eq!(run(&prog, &[5]), Ok(4));
        assert_eq!(run(&prog, &[0]), Ok(100));
        // And SWAP behaves.
        let swap_prog =
            asm::concat(&[asm::push(3), asm::push(10), vec![op::SWAP, op::DIV, op::HALT]]);
        assert_eq!(run(&swap_prog, &[]), Ok(3)); // SWAP makes it 10 / 3 == 3
    }

    #[test]
    fn journal_struct_matches_envelope_spec() {
        let j = journal::Journal {
            image_id: [0u8; 32],
            market_params_hash: [1u8; 32],
            submission_hash: [2u8; 32],
            verdict: false,
        };
        assert!(!j.verdict);
    }

    // -- input-manifest layer (toon-meta#121 / capability-market#4) ---------

    #[test]
    fn evaluate_binds_manifest_hash_not_raw_params() {
        let manifest_bytes = encode_manifest(&p1_identity(), &p2_square(), 1_735_689_600);
        let submission = encode_input(&[2]); // divergent input (2 vs 4)
        let j = evaluate([6u8; 32], &manifest_bytes, &submission);
        assert!(j.verdict);
        assert_eq!(j.market_params_hash, journal::sha256(&manifest_bytes));
        assert_ne!(
            j.market_params_hash,
            journal::sha256(&p1_identity()),
            "must bind the manifest, not a raw program blob"
        );
        assert_eq!(j.submission_hash, journal::sha256(&submission));
    }

    #[test]
    fn check_manifest_extracts_programs_by_name() {
        let m = encode_manifest(&p1_identity(), &p2_square(), 0);
        assert!(check_manifest(&m, &encode_input(&[2]))); // diverges
        assert!(!check_manifest(&m, &encode_input(&[1]))); // fixed point: agrees
    }

    #[test]
    fn malformed_manifest_is_false_not_panic() {
        let sub = encode_input(&[2]);
        assert!(!check_manifest(b"", &sub));
        assert!(!check_manifest(b"garbage", &sub));
        // Missing a program entry.
        let one_prog = manifest::encode(&[
            manifest::Entry::value(MANIFEST_PROGRAM_1, p1_identity()),
            manifest::Entry::slot("submission"),
        ])
        .unwrap();
        assert!(!check_manifest(&one_prog, &sub));
    }

    #[test]
    fn wrong_step_budget_declaration_rejected() {
        // A manifest advertising a step budget the interpreter doesn't enforce
        // must be rejected (verdict false), even with valid programs + input.
        let bad = manifest::encode(&[
            manifest::Entry::value(MANIFEST_PROGRAM_1, p1_identity()),
            manifest::Entry::value(MANIFEST_PROGRAM_2, p2_square()),
            manifest::Entry::value(MANIFEST_STEP_BUDGET, (MAX_STEPS + 1).to_le_bytes().to_vec()),
            manifest::Entry::slot("submission"),
        ])
        .unwrap();
        assert!(!check_manifest(&bad, &encode_input(&[2])));
    }
}
