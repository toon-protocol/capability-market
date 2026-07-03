// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CapabilityMarketBase} from "./CapabilityMarketBase.t.sol";
import {CapabilityMarket} from "../src/CapabilityMarket.sol";
import {IERC20} from "../src/interfaces/IERC20.sol";
import {IRiscZeroVerifier} from "../src/interfaces/IRiscZeroVerifier.sol";
import {ReentrantUSDC} from "./mocks/ReentrantUSDC.sol";
import {MockRiscZeroVerifier} from "./mocks/MockRiscZeroVerifier.sol";

/// @notice Adversarial / attacker-with-stake regression suite. Each test encodes a concrete
///         hostile scenario from the #120/#119/#121 threat model and pins the safe behaviour.
contract CapabilityMarketAdversarialTest is CapabilityMarketBase {
    // -----------------------------------------------------------------
    // State-machine boundary races: reveal vs settleTimeout
    // -----------------------------------------------------------------

    /// At the exact instant t == deadline + commitRevealWindow, reveal is still open and
    /// settleTimeout is not yet — they are never simultaneously callable, so no two-path race
    /// can double-resolve a market in one block.
    function test_boundary_revealWinsAtExactWindowEnd_settleTimeoutRevertsSameBlock() public {
        uint256 id = createDefaultMarket();
        setupStakedAndCommitted(id);

        vm.warp(deadline + commitRevealWindow); // the exact boundary

        // settleTimeout is not yet permitted at the boundary...
        vm.prank(keeper);
        vm.expectRevert(CapabilityMarket.TimeoutWindowNotElapsed.selector);
        market.settleTimeout(id);

        // ...and reveal succeeds in the very same block.
        revealAs(miner, id, validJournal());
        assertEq(uint256(market.getMarket(id).resolution), uint256(CapabilityMarket.Resolution.ResolvedYes));
    }

    /// One second past the window the sides flip: reveal is closed, settleTimeout is the only
    /// path. Again mutually exclusive.
    function test_boundary_settleTimeoutWinsPastWindow_revealRevertsSameBlock() public {
        uint256 id = createDefaultMarket();
        setupStakedAndCommitted(id);

        vm.warp(deadline + commitRevealWindow + 1);

        // reveal is closed...
        expectRevealRevert(CapabilityMarket.RevealWindowClosed.selector, miner, id, validJournal());

        // ...settleTimeout resolves NO in the same block.
        vm.prank(keeper);
        market.settleTimeout(id);
        assertEq(uint256(market.getMarket(id).resolution), uint256(CapabilityMarket.Resolution.ResolvedNo));
    }

    // -----------------------------------------------------------------
    // Commit overwrite / salt-rotation semantics
    // -----------------------------------------------------------------

    /// A committer may overwrite their own commitment (e.g. salt rotation). After overwriting,
    /// the OLD preimage no longer reveals — only the current commitment does. This only ever
    /// affects the committer themselves; it is not a griefing vector against anyone else.
    function test_commit_selfOverwrite_oldPreimageDead_newPreimageReveals() public {
        uint256 id = createDefaultMarket();
        setupStakedAndCommitted(id); // miner already committed with `salt`

        bytes32 newSalt = keccak256("rotated-salt");
        bytes32 newCommitment = keccak256(abi.encodePacked(solutionHash, solutionArweaveTx, miner, newSalt));
        vm.prank(miner);
        market.commit(id, newCommitment);

        bytes memory journal = validJournal();

        // Old salt no longer matches the stored (rotated) commitment.
        bytes memory seal = sealFor(journal);
        vm.expectRevert(CapabilityMarket.CommitmentMismatch.selector);
        vm.prank(miner);
        market.reveal(id, solutionHash, solutionArweaveTx, salt, seal, journal);

        // New salt reveals cleanly.
        seal = sealFor(journal);
        vm.prank(miner);
        market.reveal(id, solutionHash, solutionArweaveTx, newSalt, seal, journal);
        assertEq(uint256(market.getMarket(id).resolution), uint256(CapabilityMarket.Resolution.ResolvedYes));
    }

    /// Commitments are keyed per (market, sender): an attacker committing on the same market
    /// cannot touch the victim's commitment slot, so the victim still reveals normally.
    function test_commit_attackerCannotClobberVictimCommitment() public {
        uint256 id = createDefaultMarket();
        setupStakedAndCommitted(id); // miner committed

        // Attacker commits garbage on the same market from their own address.
        vm.warp(lockWindowEnd + 1);
        vm.prank(frontrunner);
        market.commit(id, keccak256("attacker-garbage"));

        // Victim's commitment is untouched; reveal succeeds.
        revealAs(miner, id, validJournal());
        assertEq(market.getMarket(id).winner, miner);
    }

    // -----------------------------------------------------------------
    // Multiple committers, first valid reveal wins
    // -----------------------------------------------------------------

    /// The spec allows several miners committing; the first valid reveal resolves the market
    /// and every later reveal — even from a legitimately-committed second miner — reverts.
    function test_multipleCommitters_firstRevealWins_laterRevealReverts() public {
        uint256 id = createDefaultMarket();
        stakeAs(alice, id, CapabilityMarket.Side.YES, 100 * USDC_UNIT);
        stakeAs(carol, id, CapabilityMarket.Side.NO, 100 * USDC_UNIT);
        vm.warp(lockWindowEnd + 1);

        // Two independent miners both commit to the same public solution (each binds its own
        // address, so the two commitment hashes differ).
        commitAs(miner, id);
        commitAs(bob, id);

        // miner reveals first -> ResolvedYes, winner = miner.
        revealAs(miner, id, validJournal());
        assertEq(market.getMarket(id).winner, miner);

        // bob, though validly committed, is too late.
        expectRevealRevert(CapabilityMarket.AlreadyResolved.selector, bob, id, validJournal());
    }

    // -----------------------------------------------------------------
    // Journal / cross-market binding
    // -----------------------------------------------------------------

    /// A (proof, journal) pair from one market cannot be replayed against a market that pins a
    /// DIFFERENT marketParamsHash — the field check rejects it. (imageId + marketParamsHash +
    /// submissionHash all bind the journal to the proposition.)
    function test_crossMarketReplay_differentParamsHash_reverts() public {
        // Market A: default params. Resolve it so its journal/seal are "public".
        uint256 idA = createDefaultMarket();
        setupStakedAndCommitted(idA);
        bytes memory journalA = validJournal();
        revealAs(miner, idA, journalA);

        // Market B: same imageId but a DIFFERENT marketParamsHash, with its own fresh windows
        // (A's are now in the past). Its commit window is opened below.
        bytes32 otherParams = keccak256("different-market-params");
        uint256 bLock = block.timestamp + 1 days;
        uint256 bDeadline = block.timestamp + 2 days;
        vm.prank(creator);
        uint256 idB = market.createMarket(
            imageId, predicateArweaveTx, otherParams, bDeadline, commitRevealWindow, bLock, bountyBps, 0
        );
        stakeAs(alice, idB, CapabilityMarket.Side.YES, 100 * USDC_UNIT);
        vm.warp(bLock + 1);
        // Attacker commits on B binding the (now public) solution to their own address.
        vm.prank(frontrunner);
        market.commit(idB, keccak256(abi.encodePacked(solutionHash, solutionArweaveTx, frontrunner, salt)));

        // Replaying A's journal against B fails the marketParamsHash field check.
        bytes memory seal = sealFor(journalA);
        vm.expectRevert(CapabilityMarket.JournalMarketParamsHashMismatch.selector);
        vm.prank(frontrunner);
        market.reveal(idB, solutionHash, solutionArweaveTx, salt, seal, journalA);
    }

    /// Documents the accepted design property (envelope spec: the journal has NO marketId
    /// field, by construction): two markets that pin an IDENTICAL (imageId, marketParamsHash)
    /// are the same proposition, so a proof for one is legitimately valid for the other. This
    /// is not a fund-safety hole — the submission bytes are public post-reveal and the proof is
    /// deterministically re-derivable, so binding marketId would not stop a copier; it would
    /// only break byte-for-byte Rust/Solidity journal conformance (#121). Front-running WITHIN
    /// a market is still defended by the per-(market,sender) commitment.
    function test_crossMarketReplay_identicalProposition_isAcceptedByDesign() public {
        uint256 idA = createDefaultMarket();
        setupStakedAndCommitted(idA);
        bytes memory journalA = validJournal();
        revealAs(miner, idA, journalA);

        // Market B: byte-identical predicate + params (a duplicate proposition), with its own
        // fresh windows since A's have elapsed.
        uint256 bLock = block.timestamp + 1 days;
        uint256 bDeadline = block.timestamp + 2 days;
        vm.prank(creator);
        uint256 idB = market.createMarket(
            imageId, predicateArweaveTx, marketParamsHash, bDeadline, commitRevealWindow, bLock, bountyBps, 0
        );
        stakeAs(alice, idB, CapabilityMarket.Side.YES, 100 * USDC_UNIT);
        vm.warp(bLock + 1);
        vm.prank(frontrunner);
        market.commit(idB, keccak256(abi.encodePacked(solutionHash, solutionArweaveTx, frontrunner, salt)));

        // Same proposition => the proof is genuinely valid for B too. Resolves ResolvedYes.
        bytes memory seal = sealFor(journalA);
        vm.prank(frontrunner);
        market.reveal(idB, solutionHash, solutionArweaveTx, salt, seal, journalA);
        assertEq(uint256(market.getMarket(idB).resolution), uint256(CapabilityMarket.Resolution.ResolvedYes));
    }

    // -----------------------------------------------------------------
    // Staking side / lifecycle guards
    // -----------------------------------------------------------------

    /// No staking is possible after a market has resolved (the lock window is long past).
    function test_stake_afterResolution_reverts() public {
        uint256 id = createDefaultMarket();
        setupStakedAndCommitted(id);
        revealAs(miner, id, validJournal());

        vm.prank(alice);
        vm.expectRevert(CapabilityMarket.StakingClosed.selector);
        market.stake(id, CapabilityMarket.Side.YES, 1 * USDC_UNIT);
    }

    /// seedNoStake makes the creator a normal NO staker: the seed is an ordinary withdrawable
    /// stake (never locked), and it side-locks the creator to NO like any other staker.
    function test_seedNoStake_creatorIsNormalNoStaker_sideLockedAndWithdrawable() public {
        uint256 seed = 1000 * USDC_UNIT;
        uint256 id = createMarketWithSeed(seed);

        // Recorded as a normal NO stake owned by the creator.
        CapabilityMarket.Stake memory s = market.getStake(id, creator);
        assertEq(uint256(s.side), uint256(CapabilityMarket.Side.NO));
        assertEq(s.amount, seed);
        assertFalse(s.withdrawn);

        // Side-locked: the creator cannot add a YES stake on top of the seed.
        vm.prank(creator);
        vm.expectRevert(CapabilityMarket.SideMismatch.selector);
        market.stake(id, CapabilityMarket.Side.YES, 1 * USDC_UNIT);

        // On a NO win (timeout), the creator withdraws the seed like any other NO staker.
        vm.warp(deadline + commitRevealWindow + 1);
        vm.prank(keeper);
        market.settleTimeout(id);

        uint256 before = usdc.balanceOf(creator);
        vm.prank(creator);
        market.withdraw(id);
        // Sole NO staker, bounty deducted: gets pool minus keeper bounty back.
        uint256 bounty = seed * bountyBps / 10_000;
        assertEq(usdc.balanceOf(creator) - before, seed - bounty);
    }

    // -----------------------------------------------------------------
    // Reentrancy on the USDC payout path
    // -----------------------------------------------------------------

    /// A malicious staking token that reenters withdraw during its payout transfer cannot
    /// double-withdraw: the nonReentrant guard reverts the reentrant call (checks-effects-
    /// interactions already flipped `withdrawn` before the transfer), and the honest caller is
    /// paid exactly once.
    function test_withdraw_reentrantToken_cannotDoubleWithdraw() public {
        ReentrantUSDC evil = new ReentrantUSDC();
        MockRiscZeroVerifier v = new MockRiscZeroVerifier();
        CapabilityMarket m = new CapabilityMarket(IERC20(address(evil)), IRiscZeroVerifier(address(v)));

        // Fund + approve the attacker (a YES staker) and a NO staker.
        evil.mint(frontrunner, 1_000_000 * USDC_UNIT);
        evil.mint(carol, 1_000_000 * USDC_UNIT);
        vm.prank(frontrunner);
        evil.approve(address(m), type(uint256).max);
        vm.prank(carol);
        evil.approve(address(m), type(uint256).max);

        vm.prank(creator);
        uint256 id = m.createMarket(
            imageId, predicateArweaveTx, marketParamsHash, deadline, commitRevealWindow, lockWindowEnd, 0, 0
        );

        vm.prank(frontrunner);
        m.stake(id, CapabilityMarket.Side.YES, 100 * USDC_UNIT);
        vm.prank(carol);
        m.stake(id, CapabilityMarket.Side.NO, 100 * USDC_UNIT);

        vm.warp(lockWindowEnd + 1);
        bytes32 commitment = keccak256(abi.encodePacked(solutionHash, solutionArweaveTx, frontrunner, salt));
        vm.prank(frontrunner);
        m.commit(id, commitment);

        bytes memory journal = validJournal();
        bytes memory seal = v.mockSeal(imageId, sha256(journal));
        vm.prank(frontrunner);
        m.reveal(id, solutionHash, solutionArweaveTx, salt, seal, journal);

        // Winner is the sole YES staker: entitled to the whole 200 USDC pool.
        uint256 entitled = m.getWithdrawable(id, frontrunner);
        assertEq(entitled, 200 * USDC_UNIT);

        // Arm the reentrancy and withdraw once.
        evil.arm(m, id);
        uint256 before = evil.balanceOf(frontrunner);
        vm.prank(frontrunner);
        m.withdraw(id);

        // The reentrant attempt happened and was rejected; payout occurred exactly once.
        assertEq(evil.reentryAttempts(), 1, "reentry should have been attempted");
        assertEq(evil.reentryReverts(), 1, "reentrant withdraw must revert");
        assertEq(evil.balanceOf(frontrunner) - before, entitled, "attacker paid exactly once");
        assertEq(m.getWithdrawable(id, frontrunner), 0, "nothing left to withdraw");
    }
}
