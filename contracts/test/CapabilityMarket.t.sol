// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {CapabilityMarketBase} from "./CapabilityMarketBase.t.sol";
import {CapabilityMarket} from "../src/CapabilityMarket.sol";
import {MockRiscZeroVerifier} from "./mocks/MockRiscZeroVerifier.sol";

contract CreateMarketTest is CapabilityMarketBase {
    function test_createMarket_storesAllParams() public {
        uint256 id = createDefaultMarket();
        assertEq(id, 0);
        assertEq(market.marketCount(), 1);

        CapabilityMarket.Market memory m = market.getMarket(id);
        assertEq(m.creator, creator);
        assertEq(m.imageId, imageId);
        assertEq(m.predicateArweaveTx, predicateArweaveTx);
        assertEq(m.marketParamsHash, marketParamsHash);
        assertEq(m.deadline, deadline);
        assertEq(m.commitRevealWindow, commitRevealWindow);
        assertEq(m.lockWindowEnd, lockWindowEnd);
        assertEq(m.resolutionBountyBps, bountyBps);
        assertEq(m.yesPool, 0);
        assertEq(m.noPool, 0);
        assertEq(uint256(m.resolution), uint256(CapabilityMarket.Resolution.Unresolved));
        assertEq(m.winner, address(0));
        assertEq(m.bountyPaid, 0);
    }

    function test_createMarket_incrementsIds() public {
        assertEq(createDefaultMarket(), 0);
        assertEq(createDefaultMarket(), 1);
        assertEq(market.marketCount(), 2);
    }

    function test_createMarket_seedNoStake_pullsUsdcIntoNoPool() public {
        uint256 seed = 500 * USDC_UNIT;
        uint256 before = usdc.balanceOf(creator);

        uint256 id = createMarketWithSeed(seed);

        assertEq(usdc.balanceOf(creator), before - seed);
        assertEq(usdc.balanceOf(address(market)), seed);

        CapabilityMarket.Market memory m = market.getMarket(id);
        assertEq(m.noPool, seed);
        assertEq(m.yesPool, 0);

        CapabilityMarket.Stake memory s = market.getStake(id, creator);
        assertEq(uint256(s.side), uint256(CapabilityMarket.Side.NO));
        assertEq(s.amount, seed);
        assertFalse(s.withdrawn);
    }

    function test_createMarket_zeroSeed_pullsNothing() public {
        uint256 id = createMarketWithSeed(0);
        assertEq(usdc.balanceOf(address(market)), 0);
        assertEq(market.getStake(id, creator).amount, 0);
    }

    // -- #121 on-chain eligibility checks ---------------------------------

    function test_createMarket_revertsOnZeroImageId() public {
        vm.expectRevert(CapabilityMarket.ImageIdZero.selector);
        vm.prank(creator);
        market.createMarket(
            bytes32(0), predicateArweaveTx, marketParamsHash, deadline, commitRevealWindow, lockWindowEnd, bountyBps, 0
        );
    }

    function test_createMarket_revertsOnZeroMarketParamsHash() public {
        vm.expectRevert(CapabilityMarket.MarketParamsHashZero.selector);
        vm.prank(creator);
        market.createMarket(
            imageId, predicateArweaveTx, bytes32(0), deadline, commitRevealWindow, lockWindowEnd, bountyBps, 0
        );
    }

    function test_createMarket_revertsOnPastDeadline() public {
        vm.expectRevert(CapabilityMarket.DeadlineNotInFuture.selector);
        vm.prank(creator);
        market.createMarket(
            imageId,
            predicateArweaveTx,
            marketParamsHash,
            block.timestamp,
            commitRevealWindow,
            lockWindowEnd,
            bountyBps,
            0
        );
    }

    function test_createMarket_revertsWhenLockWindowNotBeforeDeadline() public {
        vm.expectRevert(CapabilityMarket.LockWindowNotBeforeDeadline.selector);
        vm.prank(creator);
        market.createMarket(
            imageId, predicateArweaveTx, marketParamsHash, deadline, commitRevealWindow, deadline, bountyBps, 0
        );
    }

    function test_createMarket_revertsOnZeroCommitRevealWindow() public {
        vm.expectRevert(CapabilityMarket.CommitRevealWindowZero.selector);
        vm.prank(creator);
        market.createMarket(imageId, predicateArweaveTx, marketParamsHash, deadline, 0, lockWindowEnd, bountyBps, 0);
    }

    function test_createMarket_revertsOnBountyAboveMax() public {
        uint256 tooHigh = market.MAX_BOUNTY_BPS() + 1; // hoisted: call must precede expectRevert
        vm.expectRevert(CapabilityMarket.BountyTooHigh.selector);
        vm.prank(creator);
        market.createMarket(
            imageId, predicateArweaveTx, marketParamsHash, deadline, commitRevealWindow, lockWindowEnd, tooHigh, 0
        );
    }

    function test_createMarket_acceptsMaxBounty() public {
        vm.prank(creator);
        uint256 id = market.createMarket(
            imageId,
            predicateArweaveTx,
            marketParamsHash,
            deadline,
            commitRevealWindow,
            lockWindowEnd,
            market.MAX_BOUNTY_BPS(),
            0
        );
        assertEq(market.getMarket(id).resolutionBountyBps, 100);
    }
}

contract StakeTest is CapabilityMarketBase {
    uint256 internal id;

    function setUp() public override {
        super.setUp();
        id = createDefaultMarket();
    }

    function test_stake_yes_updatesPoolAndPullsUsdc() public {
        stakeAs(alice, id, CapabilityMarket.Side.YES, 100 * USDC_UNIT);

        assertEq(market.getMarket(id).yesPool, 100 * USDC_UNIT);
        assertEq(usdc.balanceOf(address(market)), 100 * USDC_UNIT);

        CapabilityMarket.Stake memory s = market.getStake(id, alice);
        assertEq(uint256(s.side), uint256(CapabilityMarket.Side.YES));
        assertEq(s.amount, 100 * USDC_UNIT);
    }

    function test_stake_no_updatesPool() public {
        stakeAs(carol, id, CapabilityMarket.Side.NO, 50 * USDC_UNIT);
        assertEq(market.getMarket(id).noPool, 50 * USDC_UNIT);
        assertEq(market.getMarket(id).yesPool, 0);
    }

    function test_stake_accumulatesOnSameSide() public {
        stakeAs(alice, id, CapabilityMarket.Side.YES, 100 * USDC_UNIT);
        stakeAs(alice, id, CapabilityMarket.Side.YES, 40 * USDC_UNIT);
        assertEq(market.getStake(id, alice).amount, 140 * USDC_UNIT);
        assertEq(market.getMarket(id).yesPool, 140 * USDC_UNIT);
    }

    function test_stake_revertsOnSideSwitch() public {
        stakeAs(alice, id, CapabilityMarket.Side.YES, 100 * USDC_UNIT);
        vm.expectRevert(CapabilityMarket.SideMismatch.selector);
        vm.prank(alice);
        market.stake(id, CapabilityMarket.Side.NO, 1 * USDC_UNIT);
    }

    function test_stake_revertsOnZeroAmount() public {
        vm.expectRevert(CapabilityMarket.ZeroAmount.selector);
        vm.prank(alice);
        market.stake(id, CapabilityMarket.Side.YES, 0);
    }

    function test_stake_acceptedAtLockWindowEndBoundary() public {
        vm.warp(lockWindowEnd);
        stakeAs(alice, id, CapabilityMarket.Side.YES, 1 * USDC_UNIT);
        assertEq(market.getMarket(id).yesPool, 1 * USDC_UNIT);
    }

    function test_stake_revertsAfterLockWindowEnd() public {
        vm.warp(lockWindowEnd + 1);
        vm.expectRevert(CapabilityMarket.StakingClosed.selector);
        vm.prank(alice);
        market.stake(id, CapabilityMarket.Side.YES, 1 * USDC_UNIT);
    }

    function test_stake_revertsOnNonexistentMarket() public {
        vm.expectRevert(CapabilityMarket.MarketDoesNotExist.selector);
        vm.prank(alice);
        market.stake(99, CapabilityMarket.Side.YES, 1 * USDC_UNIT);
    }

    function test_stake_revertsWithoutAllowance() public {
        address stranger = makeAddr("stranger");
        usdc.mint(stranger, 10 * USDC_UNIT);
        vm.expectRevert("insufficient allowance");
        vm.prank(stranger);
        market.stake(id, CapabilityMarket.Side.YES, 1 * USDC_UNIT);
    }
}

contract CommitTest is CapabilityMarketBase {
    uint256 internal id;

    function setUp() public override {
        super.setUp();
        id = createDefaultMarket();
    }

    function test_commit_storesHashAndTimestamp() public {
        vm.warp(lockWindowEnd + 100);
        commitAs(miner, id);

        CapabilityMarket.Commitment memory c = market.getCommitment(id, miner);
        assertEq(c.commitmentHash, commitmentFor(miner));
        assertEq(c.timestamp, lockWindowEnd + 100);
    }

    function test_commit_revertsAtOrBeforeLockWindowEnd() public {
        vm.warp(lockWindowEnd);
        vm.expectRevert(CapabilityMarket.CommitWindowClosed.selector);
        vm.prank(miner);
        market.commit(id, commitmentFor(miner));
    }

    function test_commit_acceptedAtDeadlineBoundary() public {
        vm.warp(deadline);
        commitAs(miner, id);
        assertEq(market.getCommitment(id, miner).timestamp, deadline);
    }

    function test_commit_revertsAfterDeadline() public {
        vm.warp(deadline + 1);
        vm.expectRevert(CapabilityMarket.CommitWindowClosed.selector);
        vm.prank(miner);
        market.commit(id, commitmentFor(miner));
    }

    function test_commit_revertsOnZeroHash() public {
        vm.warp(lockWindowEnd + 1);
        vm.expectRevert(CapabilityMarket.CommitmentHashZero.selector);
        vm.prank(miner);
        market.commit(id, bytes32(0));
    }

    function test_commit_overwriteUpdatesHashAndTimestamp() public {
        vm.warp(lockWindowEnd + 1);
        vm.prank(miner);
        market.commit(id, keccak256("first"));
        vm.warp(lockWindowEnd + 500);
        commitAs(miner, id);

        CapabilityMarket.Commitment memory c = market.getCommitment(id, miner);
        assertEq(c.commitmentHash, commitmentFor(miner));
        assertEq(c.timestamp, lockWindowEnd + 500);
    }

    function test_commit_multipleCommittersCoexist() public {
        vm.warp(lockWindowEnd + 1);
        commitAs(miner, id);
        commitAs(frontrunner, id);
        assertEq(market.getCommitment(id, miner).commitmentHash, commitmentFor(miner));
        assertEq(market.getCommitment(id, frontrunner).commitmentHash, commitmentFor(frontrunner));
    }

    function test_commit_revertsOnNonexistentMarket() public {
        vm.warp(lockWindowEnd + 1);
        vm.expectRevert(CapabilityMarket.MarketDoesNotExist.selector);
        vm.prank(miner);
        market.commit(99, commitmentFor(miner));
    }
}

contract RevealTest is CapabilityMarketBase {
    uint256 internal id;

    function setUp() public override {
        super.setUp();
        id = createDefaultMarket();
        setupStakedAndCommitted(id);
    }

    function test_reveal_happyPath_resolvesYesWithMsgSenderAsWinner() public {
        revealAs(miner, id, validJournal());

        CapabilityMarket.Market memory m = market.getMarket(id);
        assertEq(uint256(m.resolution), uint256(CapabilityMarket.Resolution.ResolvedYes));
        assertEq(m.winner, miner);
        assertEq(m.bountyPaid, 0); // no keeper bounty on the reveal path
    }

    function test_reveal_acceptedAtRevealWindowBoundary() public {
        vm.warp(deadline + commitRevealWindow);
        revealAs(miner, id, validJournal());
        assertEq(uint256(market.getMarket(id).resolution), uint256(CapabilityMarket.Resolution.ResolvedYes));
    }

    function test_reveal_revertsAfterRevealWindow() public {
        vm.warp(deadline + commitRevealWindow + 1);
        expectRevealRevert(CapabilityMarket.RevealWindowClosed.selector, miner, id, validJournal());
    }

    function test_reveal_revertsWithoutCommitment() public {
        expectRevealRevert(CapabilityMarket.NoCommitment.selector, keeper, id, validJournal());
    }

    function test_reveal_revertsOnWrongSalt() public {
        bytes memory journal = validJournal();
        bytes memory seal = sealFor(journal);
        vm.expectRevert(CapabilityMarket.CommitmentMismatch.selector);
        vm.prank(miner);
        market.reveal(id, solutionHash, solutionArweaveTx, keccak256("wrong-salt"), seal, journal);
    }

    function test_reveal_revertsOnWrongSolutionHash() public {
        bytes memory journal = validJournal();
        bytes memory seal = sealFor(journal);
        vm.expectRevert(CapabilityMarket.CommitmentMismatch.selector);
        vm.prank(miner);
        market.reveal(id, keccak256("other-solution"), solutionArweaveTx, salt, seal, journal);
    }

    /// Front-running defense: a bot that copies the miner's reveal calldata verbatim from the
    /// mempool fails — it never committed.
    function test_reveal_frontrunner_copyingCalldata_reverts() public {
        expectRevealRevert(CapabilityMarket.NoCommitment.selector, frontrunner, id, validJournal());
    }

    /// Even a bot that ALSO copied the miner's commitment hash during the commit window fails:
    /// msg.sender is baked into the commitment preimage.
    function test_reveal_frontrunner_copyingCommitmentHash_reverts() public {
        // frontrunner replayed the miner's commitment hash while the window was open
        vm.prank(frontrunner);
        market.commit(id, commitmentFor(miner));

        expectRevealRevert(CapabilityMarket.CommitmentMismatch.selector, frontrunner, id, validJournal());
    }

    function test_reveal_revertsOnInvalidProof() public {
        bytes memory journal = validJournal();
        vm.expectRevert(MockRiscZeroVerifier.VerificationFailed.selector);
        vm.prank(miner);
        market.reveal(id, solutionHash, solutionArweaveTx, salt, bytes("garbage-seal"), journal);
    }

    /// A valid proof for a DIFFERENT guest program: the verifier is called with the market's
    /// pinned image ID, so the seal (bound to the other image) fails verification.
    function test_reveal_revertsOnProofForDifferentImageId() public {
        bytes memory journal = validJournal();
        bytes memory foreignSeal = verifier.mockSeal(keccak256("other-image"), sha256(journal));
        vm.expectRevert(MockRiscZeroVerifier.VerificationFailed.selector);
        vm.prank(miner);
        market.reveal(id, solutionHash, solutionArweaveTx, salt, foreignSeal, journal);
    }

    /// Proof/journal are internally consistent but the journal's fields don't match the market.
    function test_reveal_revertsOnJournalImageIdMismatch() public {
        bytes memory journal = makeJournal(keccak256("evil-image"), marketParamsHash, solutionHash, true);
        expectRevealRevert(CapabilityMarket.JournalImageIdMismatch.selector, miner, id, journal);
    }

    function test_reveal_revertsOnJournalMarketParamsHashMismatch() public {
        bytes memory journal = makeJournal(imageId, keccak256("other-params"), solutionHash, true);
        expectRevealRevert(CapabilityMarket.JournalMarketParamsHashMismatch.selector, miner, id, journal);
    }

    function test_reveal_revertsOnJournalSubmissionHashMismatch() public {
        bytes memory journal = makeJournal(imageId, marketParamsHash, keccak256("other-submission"), true);
        expectRevealRevert(CapabilityMarket.JournalSubmissionHashMismatch.selector, miner, id, journal);
    }

    function test_reveal_revertsOnFalseVerdict() public {
        bytes memory journal = makeJournal(imageId, marketParamsHash, solutionHash, false);
        expectRevealRevert(CapabilityMarket.JournalVerdictFalse.selector, miner, id, journal);
    }

    function test_reveal_revertsWhenAlreadyResolved() public {
        revealAs(miner, id, validJournal());
        expectRevealRevert(CapabilityMarket.AlreadyResolved.selector, miner, id, validJournal());
    }

    function test_reveal_revertsAfterTimeoutSettlement() public {
        vm.warp(deadline + commitRevealWindow + 1);
        vm.prank(keeper);
        market.settleTimeout(id);

        // even with the clock rolled back inside the window the state gate holds
        vm.warp(deadline + commitRevealWindow);
        expectRevealRevert(CapabilityMarket.AlreadyResolved.selector, miner, id, validJournal());
    }
}

contract SettleTimeoutTest is CapabilityMarketBase {
    uint256 internal id;

    function setUp() public override {
        super.setUp();
        id = createDefaultMarket();
        setupStakedAndCommitted(id); // pool: 400 YES + 600 NO = 1000 USDC
    }

    function test_settleTimeout_revertsAtWindowBoundary() public {
        vm.warp(deadline + commitRevealWindow);
        vm.expectRevert(CapabilityMarket.TimeoutWindowNotElapsed.selector);
        vm.prank(keeper);
        market.settleTimeout(id);
    }

    function test_settleTimeout_revertsBeforeDeadline() public {
        vm.expectRevert(CapabilityMarket.TimeoutWindowNotElapsed.selector);
        vm.prank(keeper);
        market.settleTimeout(id);
    }

    function test_settleTimeout_resolvesNoAndPaysBounty() public {
        vm.warp(deadline + commitRevealWindow + 1);
        uint256 before = usdc.balanceOf(keeper);

        vm.prank(keeper);
        market.settleTimeout(id);

        CapabilityMarket.Market memory m = market.getMarket(id);
        assertEq(uint256(m.resolution), uint256(CapabilityMarket.Resolution.ResolvedNo));
        assertEq(m.winner, address(0));

        // 1% of 1000 USDC pool
        uint256 expectedBounty = 1000 * USDC_UNIT * bountyBps / 10_000;
        assertEq(m.bountyPaid, expectedBounty);
        assertEq(usdc.balanceOf(keeper) - before, expectedBounty);
    }

    function test_settleTimeout_zeroBountyBps_paysNothing() public {
        vm.warp(t0);
        vm.prank(creator);
        uint256 id2 = market.createMarket(
            imageId, predicateArweaveTx, marketParamsHash, deadline, commitRevealWindow, lockWindowEnd, 0, 0
        );
        stakeAs(alice, id2, CapabilityMarket.Side.YES, 100 * USDC_UNIT);

        vm.warp(deadline + commitRevealWindow + 1);
        uint256 before = usdc.balanceOf(keeper);
        vm.prank(keeper);
        market.settleTimeout(id2);

        assertEq(usdc.balanceOf(keeper), before);
        assertEq(market.getMarket(id2).bountyPaid, 0);
    }

    function test_settleTimeout_revertsAfterValidReveal() public {
        revealAs(miner, id, validJournal());
        vm.warp(deadline + commitRevealWindow + 1);
        vm.expectRevert(CapabilityMarket.AlreadyResolved.selector);
        vm.prank(keeper);
        market.settleTimeout(id);
    }

    function test_settleTimeout_revertsOnDoubleSettle() public {
        vm.warp(deadline + commitRevealWindow + 1);
        vm.prank(keeper);
        market.settleTimeout(id);
        vm.expectRevert(CapabilityMarket.AlreadyResolved.selector);
        vm.prank(keeper);
        market.settleTimeout(id);
    }

    function test_settleTimeout_revertsOnNonexistentMarket() public {
        vm.expectRevert(CapabilityMarket.MarketDoesNotExist.selector);
        vm.prank(keeper);
        market.settleTimeout(99);
    }
}

contract WithdrawTest is CapabilityMarketBase {
    uint256 internal id;

    function setUp() public override {
        super.setUp();
        id = createDefaultMarket();
        // alice 100 YES, bob 300 YES, carol 200 NO, dave 400 NO — total 1000 USDC
        setupStakedAndCommitted(id);
    }

    // -- YES resolution (reveal path, no bounty) ---------------------------

    function test_withdraw_yesWinners_parimutuelPayouts() public {
        revealAs(miner, id, validJournal());

        // W = 400, losersPool = 600, winnersShare = 1000
        // alice: 100 + 600*100/400 = 250; bob: 300 + 600*300/400 = 750
        assertEq(market.getWithdrawable(id, alice), 250 * USDC_UNIT);
        assertEq(market.getWithdrawable(id, bob), 750 * USDC_UNIT);
        assertEq(market.getWithdrawable(id, carol), 0);
        assertEq(market.getWithdrawable(id, dave), 0);

        uint256 aliceBefore = usdc.balanceOf(alice);
        vm.prank(alice);
        market.withdraw(id);
        assertEq(usdc.balanceOf(alice) - aliceBefore, 250 * USDC_UNIT);

        uint256 bobBefore = usdc.balanceOf(bob);
        vm.prank(bob);
        market.withdraw(id);
        assertEq(usdc.balanceOf(bob) - bobBefore, 750 * USDC_UNIT);

        // pool fully drained, nothing locked
        assertEq(usdc.balanceOf(address(market)), 0);
    }

    function test_withdraw_loser_reverts() public {
        revealAs(miner, id, validJournal());
        vm.expectRevert(CapabilityMarket.NothingToWithdraw.selector);
        vm.prank(carol);
        market.withdraw(id);
    }

    function test_withdraw_nonStakerWinner_reverts() public {
        // the winning miner never staked; without a staked position there is no payout
        revealAs(miner, id, validJournal());
        vm.expectRevert(CapabilityMarket.NothingToWithdraw.selector);
        vm.prank(miner);
        market.withdraw(id);
    }

    // -- NO resolution (timeout path, bounty deducted) ----------------------

    function test_withdraw_noWinners_afterTimeout_bountyDeducted() public {
        vm.warp(deadline + commitRevealWindow + 1);
        vm.prank(keeper);
        market.settleTimeout(id);

        // bounty = 10, winnersShare = 990, W = 600
        // carol: 200*990/600 = 330; dave: 400*990/600 = 660
        assertEq(market.getWithdrawable(id, carol), 330 * USDC_UNIT);
        assertEq(market.getWithdrawable(id, dave), 660 * USDC_UNIT);
        assertEq(market.getWithdrawable(id, alice), 0);

        vm.prank(carol);
        market.withdraw(id);
        vm.prank(dave);
        market.withdraw(id);

        // 1000 = 10 bounty + 330 + 660: fully drained
        assertEq(usdc.balanceOf(address(market)), 0);
    }

    function test_withdraw_seedNoStake_winsOnTimeout() public {
        vm.warp(t0);
        uint256 id2 = createMarketWithSeed(600 * USDC_UNIT);
        stakeAs(alice, id2, CapabilityMarket.Side.YES, 400 * USDC_UNIT);

        vm.warp(deadline + commitRevealWindow + 1);
        vm.prank(keeper);
        market.settleTimeout(id2);

        // total 1000, bounty 10, creator is the only NO staker: gets all 990
        assertEq(market.getWithdrawable(id2, creator), 990 * USDC_UNIT);
        vm.prank(creator);
        market.withdraw(id2);
    }

    // -- guards -------------------------------------------------------------

    function test_withdraw_revertsWhileUnresolved() public {
        assertEq(market.getWithdrawable(id, alice), 0);
        vm.expectRevert(CapabilityMarket.NothingToWithdraw.selector);
        vm.prank(alice);
        market.withdraw(id);
    }

    function test_withdraw_revertsOnDoubleWithdraw() public {
        revealAs(miner, id, validJournal());
        vm.prank(alice);
        market.withdraw(id);

        assertEq(market.getWithdrawable(id, alice), 0);
        vm.expectRevert(CapabilityMarket.NothingToWithdraw.selector);
        vm.prank(alice);
        market.withdraw(id);
    }

    function test_withdraw_revertsForNonStaker() public {
        revealAs(miner, id, validJournal());
        vm.expectRevert(CapabilityMarket.NothingToWithdraw.selector);
        vm.prank(keeper);
        market.withdraw(id);
    }

    function test_withdraw_revertsOnNonexistentMarket() public {
        vm.expectRevert(CapabilityMarket.MarketDoesNotExist.selector);
        vm.prank(alice);
        market.withdraw(99);
    }
}

/// Edge cases in the parimutuel math: empty winning pool, zero losers pool, bounty exceeding
/// the losing pool, rounding dust.
contract WithdrawEdgeCaseTest is CapabilityMarketBase {
    function test_emptyWinningPool_resolvedYes_losersReclaimStakes() public {
        // Only NO stakes; a non-staking miner still resolves YES with a valid proof.
        uint256 id = createDefaultMarket();
        stakeAs(carol, id, CapabilityMarket.Side.NO, 200 * USDC_UNIT);
        stakeAs(dave, id, CapabilityMarket.Side.NO, 400 * USDC_UNIT);
        vm.warp(lockWindowEnd + 1);
        commitAs(miner, id);
        revealAs(miner, id, validJournal());

        // yesPool == 0: no winner can claim, so NO stakers reclaim in full (no bounty on reveal)
        assertEq(market.getWithdrawable(id, carol), 200 * USDC_UNIT);
        assertEq(market.getWithdrawable(id, dave), 400 * USDC_UNIT);

        vm.prank(carol);
        market.withdraw(id);
        vm.prank(dave);
        market.withdraw(id);
        assertEq(usdc.balanceOf(address(market)), 0);
    }

    function test_emptyWinningPool_timeout_yesStakersReclaimMinusBounty() public {
        // Only YES stakes; timeout resolves NO but the NO pool is empty.
        uint256 id = createDefaultMarket();
        stakeAs(alice, id, CapabilityMarket.Side.YES, 100 * USDC_UNIT);
        stakeAs(bob, id, CapabilityMarket.Side.YES, 300 * USDC_UNIT);

        vm.warp(deadline + commitRevealWindow + 1);
        vm.prank(keeper);
        market.settleTimeout(id);

        // total 400, bounty 4, remainder 396 split pro-rata: alice 99, bob 297
        assertEq(market.getWithdrawable(id, alice), 99 * USDC_UNIT);
        assertEq(market.getWithdrawable(id, bob), 297 * USDC_UNIT);

        vm.prank(alice);
        market.withdraw(id);
        vm.prank(bob);
        market.withdraw(id);
        assertEq(usdc.balanceOf(address(market)), 0);
    }

    function test_zeroLosersPool_winnersGetExactStakeBack() public {
        // Only YES stakes, resolved YES via reveal: no losers, no bounty — payout == stake.
        uint256 id = createDefaultMarket();
        stakeAs(alice, id, CapabilityMarket.Side.YES, 123 * USDC_UNIT);
        vm.warp(lockWindowEnd + 1);
        commitAs(miner, id);
        revealAs(miner, id, validJournal());

        assertEq(market.getWithdrawable(id, alice), 123 * USDC_UNIT);
        vm.prank(alice);
        market.withdraw(id);
        assertEq(usdc.balanceOf(address(market)), 0);
    }

    function test_bountyExceedsLosersPool_winnersTakeHaircut_noUnderflow() public {
        // Losing (YES) pool is tiny relative to the 1% bounty on the total pool:
        // yes = 50 raw units, no = 10_000_000 raw. total = 10_000_050, bounty = 100_000 (1%).
        // winnersShare = 9_900_050 < W = 10_000_000 -> "losersPool" would be negative in the
        // naive formula; winners take a pro-rata haircut instead of the tx reverting.
        uint256 id = createDefaultMarket();
        stakeAs(alice, id, CapabilityMarket.Side.YES, 50);
        stakeAs(carol, id, CapabilityMarket.Side.NO, 10_000_000);

        vm.warp(deadline + commitRevealWindow + 1);
        vm.prank(keeper);
        market.settleTimeout(id);

        uint256 expected = uint256(10_000_000) * 9_900_050 / 10_000_000; // 9_900_050
        assertEq(market.getWithdrawable(id, carol), expected);
        vm.prank(carol);
        market.withdraw(id);

        // bounty + payout == total pool: nothing locked, nothing overdrawn
        assertEq(usdc.balanceOf(address(market)), 0);
    }

    function test_roundingDust_neverOverdraws() public {
        // Amounts chosen so the pro-rata division floors.
        uint256 id = createDefaultMarket();
        stakeAs(alice, id, CapabilityMarket.Side.YES, 3);
        stakeAs(bob, id, CapabilityMarket.Side.YES, 7);
        stakeAs(carol, id, CapabilityMarket.Side.NO, 11);
        vm.warp(lockWindowEnd + 1);
        commitAs(miner, id);
        revealAs(miner, id, validJournal());

        // winnersShare = 21, W = 10: alice = 3*21/10 = 6, bob = 7*21/10 = 14, dust = 1
        assertEq(market.getWithdrawable(id, alice), 6);
        assertEq(market.getWithdrawable(id, bob), 14);

        vm.prank(alice);
        market.withdraw(id);
        vm.prank(bob);
        market.withdraw(id);

        assertEq(usdc.balanceOf(address(market)), 1); // dust stays, never overdraws
    }
}

/// The #120 acceptance lifecycle: 5 YES + 5 NO stakers, commit, reveal, winners withdraw.
contract LifecycleTest is CapabilityMarketBase {
    address[5] internal yesStakers;
    address[5] internal noStakers;

    function setUp() public override {
        super.setUp();
        for (uint256 i = 0; i < 5; i++) {
            yesStakers[i] = makeAddr(string(abi.encodePacked("yes", i)));
            noStakers[i] = makeAddr(string(abi.encodePacked("no", i)));
            usdc.mint(yesStakers[i], 1000 * USDC_UNIT);
            usdc.mint(noStakers[i], 1000 * USDC_UNIT);
            vm.prank(yesStakers[i]);
            usdc.approve(address(market), type(uint256).max);
            vm.prank(noStakers[i]);
            usdc.approve(address(market), type(uint256).max);
        }
    }

    function test_fullLifecycle_revealPath() public {
        uint256 id = createDefaultMarket();

        uint256 yesTotal;
        uint256 noTotal;
        for (uint256 i = 0; i < 5; i++) {
            uint256 yesAmt = (i + 1) * 10 * USDC_UNIT;
            uint256 noAmt = (i + 1) * 20 * USDC_UNIT;
            stakeAs(yesStakers[i], id, CapabilityMarket.Side.YES, yesAmt);
            stakeAs(noStakers[i], id, CapabilityMarket.Side.NO, noAmt);
            yesTotal += yesAmt;
            noTotal += noAmt;
        }
        assertEq(market.getMarket(id).yesPool, yesTotal);
        assertEq(market.getMarket(id).noPool, noTotal);

        vm.warp(lockWindowEnd + 1);
        commitAs(miner, id);
        vm.warp(deadline + 100); // reveal inside the grace window
        revealAs(miner, id, validJournal());

        uint256 total = yesTotal + noTotal;
        uint256 paidOut;
        for (uint256 i = 0; i < 5; i++) {
            uint256 s = (i + 1) * 10 * USDC_UNIT;
            uint256 expected = s + noTotal * s / yesTotal;
            assertEq(market.getWithdrawable(id, yesStakers[i]), expected);

            uint256 before = usdc.balanceOf(yesStakers[i]);
            vm.prank(yesStakers[i]);
            market.withdraw(id);
            paidOut += usdc.balanceOf(yesStakers[i]) - before;

            // losers cannot withdraw
            vm.expectRevert(CapabilityMarket.NothingToWithdraw.selector);
            vm.prank(noStakers[i]);
            market.withdraw(id);
        }
        assertLe(paidOut, total);
        assertEq(usdc.balanceOf(address(market)), total - paidOut);
    }

    function test_fullLifecycle_timeoutPath() public {
        uint256 id = createDefaultMarket();
        uint256 yesTotal;
        uint256 noTotal;
        for (uint256 i = 0; i < 5; i++) {
            uint256 yesAmt = (i + 1) * 10 * USDC_UNIT;
            uint256 noAmt = (i + 1) * 20 * USDC_UNIT;
            stakeAs(yesStakers[i], id, CapabilityMarket.Side.YES, yesAmt);
            stakeAs(noStakers[i], id, CapabilityMarket.Side.NO, noAmt);
            yesTotal += yesAmt;
            noTotal += noAmt;
        }

        // nobody reveals; keeper settles after the window
        vm.warp(deadline + commitRevealWindow + 1);
        vm.prank(keeper);
        market.settleTimeout(id);

        uint256 total = yesTotal + noTotal;
        uint256 bounty = total * bountyBps / 10_000;
        uint256 winnersShare = total - bounty;

        uint256 paidOut = bounty;
        for (uint256 i = 0; i < 5; i++) {
            uint256 s = (i + 1) * 20 * USDC_UNIT;
            uint256 expected = s * winnersShare / noTotal;
            assertEq(market.getWithdrawable(id, noStakers[i]), expected);

            uint256 before = usdc.balanceOf(noStakers[i]);
            vm.prank(noStakers[i]);
            market.withdraw(id);
            paidOut += usdc.balanceOf(noStakers[i]) - before;
        }
        assertLe(paidOut, total);
    }
}
