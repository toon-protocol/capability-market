//! Adversarial review tests (toon-meta#122 review step 4): properties an
//! attacker would probe that the unit suite does not already pin down.
//!
//! The money-critical property is soundness of PASS: `check(p1, p2, x)`
//! must never return `true` unless both programs really halted cleanly with
//! different outputs. Determinism of the interpreter is what makes the
//! self-difftest property below airtight: a program can never diverge from
//! itself, no matter what bytecode or input is thrown at it.

use difftest::{asm, check, op, run, verdict, ExecError, MAX_STACK};

/// Deterministic xorshift32 so failures reproduce exactly.
fn xorshift(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

/// 20k random byte blobs as programs: the interpreter must never panic, and
/// a program run against ITSELF must never be judged divergent (this is the
/// determinism meta-property — any nondeterministic op, uninitialized read,
/// or state leak between the two runs would eventually fail it).
#[test]
fn fuzz_random_bytecode_never_panics_and_never_self_diverges() {
    let mut rng = 0x1234_5678u32;
    for _ in 0..20_000 {
        let len = (xorshift(&mut rng) % 81) as usize; // 0..=80 bytes
        let program: Vec<u8> = (0..len)
            .map(|_| (xorshift(&mut rng) & 0xff) as u8)
            .collect();
        let words = (xorshift(&mut rng) % 4) as usize;
        let input: Vec<u64> = (0..words)
            .map(|_| xorshift(&mut rng) as u64 | ((xorshift(&mut rng) as u64) << 32))
            .collect();
        // Must not panic, whatever it returns.
        let _ = run(&program, &input);
        // A program NEVER diverges from itself.
        assert!(
            !check(&program, &program, &input),
            "self-difftest diverged: program={program:02x?} input={input:?}"
        );
    }
}

/// Same meta-property through the raw-bytes entry point.
#[test]
fn fuzz_verdict_raw_bytes_never_panics() {
    let mut rng = 0x9e37_79b9u32;
    for _ in 0..5_000 {
        let plen = (xorshift(&mut rng) % 40) as usize;
        let p: Vec<u8> = (0..plen).map(|_| (xorshift(&mut rng) & 0xff) as u8).collect();
        let ilen = (xorshift(&mut rng) % 24) as usize; // deliberately often unaligned
        let i: Vec<u8> = (0..ilen).map(|_| (xorshift(&mut rng) & 0xff) as u8).collect();
        let _ = verdict(&p, &p, &i);
        assert!(!verdict(&p, &p, &(0..8).map(|_| 0u8).collect::<Vec<_>>()));
    }
}

/// Byte-addressed jump INTO the middle of a PUSH immediate: the immediate
/// bytes are then decoded as opcodes. Legal (jumps are byte offsets), must
/// be deterministic and panic-free — and still can't make a program diverge
/// from itself.
#[test]
fn jump_into_immediate_is_deterministic_not_panic() {
    // PUSH 0x0c0c_0c0c_0c0c_0c0c (immediate bytes are all HALT opcodes),
    // then JMP 3 — lands inside the immediate, executes HALT with the
    // pushed value on the stack.
    let mut program = asm::push(0x0c0c_0c0c_0c0c_0c0cu64);
    program.extend_from_slice(&asm::jmp(3));
    let first = run(&program, &[]);
    assert_eq!(first, Ok(0x0c0c_0c0c_0c0c_0c0c));
    assert_eq!(run(&program, &[]), first, "must be deterministic");
    assert!(!check(&program, &program, &[]));
}

/// The stack ceiling must trip at exactly MAX_STACK, before any deeper push.
#[test]
fn stack_ceiling_exact() {
    // MAX_STACK pushes succeed; one more traps.
    let mut ok = Vec::new();
    for _ in 0..MAX_STACK {
        ok.extend_from_slice(&asm::push(1));
    }
    ok.push(op::HALT);
    assert_eq!(run(&ok, &[]), Ok(1));

    let mut over = Vec::new();
    for _ in 0..=MAX_STACK {
        over.extend_from_slice(&asm::push(1));
    }
    over.push(op::HALT);
    assert!(matches!(run(&over, &[]), Err(ExecError::StackOverflow { .. })));
}

/// PASS requires BOTH sides clean: a trapping program must poison the
/// verdict even when the other side halts with a different-looking output,
/// in both argument orders.
#[test]
fn trap_on_either_side_never_passes() {
    let clean = asm::concat(&[asm::push(42), vec![op::HALT]]);
    let traps: Vec<Vec<u8>> = vec![
        vec![op::POP],                     // stack underflow
        vec![op::HALT],                    // HALT on empty stack
        vec![0xff],                        // unknown opcode
        asm::push(7),                      // runs off the end
        asm::jmp(0),                       // infinite loop -> step budget
        asm::concat(&[asm::push(1), asm::push(0), vec![op::DIV, op::HALT]]),
    ];
    for t in &traps {
        assert!(run(t, &[]).is_err(), "expected trap: {t:02x?}");
        assert!(!check(t, &clean, &[]), "trap left must fail: {t:02x?}");
        assert!(!check(&clean, t, &[]), "trap right must fail: {t:02x?}");
    }
}
