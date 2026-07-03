// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {CapabilityMarketBase} from "./CapabilityMarketBase.t.sol";
import {CapabilityMarket} from "../src/CapabilityMarket.sol";

/// Fuzz tests on the parimutuel payout math. Core properties, for both resolution paths and
/// arbitrary stake distributions:
///   1. conservation: keeper bounty + sum of all withdrawals <= total pool (no overdraw)
///   2. no lockup: what remains after every entitled party withdraws is only rounding dust,
///      strictly less than one raw unit per staker
///   3. every entitled withdrawal actually succeeds (the contract always holds enough USDC)
contract CapabilityMarketFuzzTest is CapabilityMarketBase {
    uint256 internal constant N = 8;
    address[N] internal actors;

    function setUp() public override {
        super.setUp();
        for (uint256 i = 0; i < N; i++) {
            actors[i] = makeAddr(string(abi.encodePacked("actor", i)));
            usdc.mint(actors[i], type(uint96).max);
            vm.prank(actors[i]);
            usdc.approve(address(market), type(uint256).max);
        }
    }

    function _setupFuzzMarket(uint96[N] memory amounts, bool[N] memory sides, uint256 bountyBpsSeed)
        internal
        returns (uint256 id, uint256 total)
    {
        bountyBps = bound(bountyBpsSeed, 0, market.MAX_BOUNTY_BPS());
        vm.prank(creator);
        id = market.createMarket(
            imageId, predicateArweaveTx, marketParamsHash, deadline, commitRevealWindow, lockWindowEnd, bountyBps, 0
        );

        for (uint256 i = 0; i < N; i++) {
            uint256 amount = bound(uint256(amounts[i]), 0, type(uint96).max);
            if (amount == 0) continue;
            stakeAs(actors[i], id, sides[i] ? CapabilityMarket.Side.YES : CapabilityMarket.Side.NO, amount);
            total += amount;
        }
    }

    function _withdrawAllAndCheck(uint256 id, uint256 total, uint256 bountyPaid) internal {
        uint256 sumPayouts;
        for (uint256 i = 0; i < N; i++) {
            uint256 entitled = market.getWithdrawable(id, actors[i]);
            if (entitled == 0) continue;

            uint256 before = usdc.balanceOf(actors[i]);
            vm.prank(actors[i]);
            market.withdraw(id); // must never revert for an entitled staker (no overdraw)
            uint256 got = usdc.balanceOf(actors[i]) - before;
            assertEq(got, entitled, "payout != quoted withdrawable");
            sumPayouts += got;

            assertEq(market.getWithdrawable(id, actors[i]), 0, "withdrawable after withdraw");
        }

        // 1. conservation
        assertLe(bountyPaid + sumPayouts, total, "paid out more than the pool");
        // 2. no lockup beyond rounding dust (< 1 raw unit per staker)
        uint256 locked = total - bountyPaid - sumPayouts;
        assertLe(locked, N, "more than dust locked");
        assertEq(usdc.balanceOf(address(market)), locked, "balance != expected dust");
    }

    /// Resolution via reveal (YES wins, no keeper bounty).
    function testFuzz_parimutuel_revealPath(uint96[N] memory amounts, bool[N] memory sides, uint256 bountyBpsSeed)
        public
    {
        (uint256 id, uint256 total) = _setupFuzzMarket(amounts, sides, bountyBpsSeed);

        vm.warp(lockWindowEnd + 1);
        commitAs(miner, id);
        revealAs(miner, id, validJournal());

        assertEq(market.getMarket(id).bountyPaid, 0);
        _withdrawAllAndCheck(id, total, 0);
    }

    /// Resolution via timeout (NO wins, keeper takes resolutionBountyBps of the pool).
    function testFuzz_parimutuel_timeoutPath(uint96[N] memory amounts, bool[N] memory sides, uint256 bountyBpsSeed)
        public
    {
        (uint256 id, uint256 total) = _setupFuzzMarket(amounts, sides, bountyBpsSeed);

        vm.warp(deadline + commitRevealWindow + 1);
        uint256 keeperBefore = usdc.balanceOf(keeper);
        vm.prank(keeper);
        market.settleTimeout(id);
        uint256 bountyPaid = usdc.balanceOf(keeper) - keeperBefore;

        assertEq(bountyPaid, total * bountyBps / 10_000, "bounty formula");
        assertEq(bountyPaid, market.getMarket(id).bountyPaid);
        _withdrawAllAndCheck(id, total, bountyPaid);
    }

    /// A winner's payout is never less than the floor of their proportional entitlement, and
    /// winners with equal stakes receive equal payouts.
    function testFuzz_parimutuel_fairness(uint96 stakeA, uint96 stakeB, uint96 stakeLoser) public {
        uint256 a = bound(uint256(stakeA), 1, type(uint96).max);
        uint256 b = bound(uint256(stakeB), 1, type(uint96).max);
        uint256 l = bound(uint256(stakeLoser), 1, type(uint96).max);

        uint256 id = createDefaultMarket();
        stakeAs(actors[0], id, CapabilityMarket.Side.YES, a);
        stakeAs(actors[1], id, CapabilityMarket.Side.YES, b);
        stakeAs(actors[2], id, CapabilityMarket.Side.YES, a); // same stake as actors[0]
        stakeAs(actors[3], id, CapabilityMarket.Side.NO, l);

        vm.warp(lockWindowEnd + 1);
        commitAs(miner, id);
        revealAs(miner, id, validJournal());

        uint256 w = 2 * a + b;
        uint256 total = w + l;
        uint256 payoutA = market.getWithdrawable(id, actors[0]);

        // equal stakes -> equal payouts
        assertEq(payoutA, market.getWithdrawable(id, actors[2]));
        // exact parimutuel formula: s * winnersShare / W (winnersShare == total, no bounty)
        assertEq(payoutA, a * total / w);
        // winners never lose money on the reveal path (no bounty): payout >= own stake
        assertGe(payoutA, a);
    }
}
