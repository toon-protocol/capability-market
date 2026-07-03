// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CapabilityMarket} from "../src/CapabilityMarket.sol";
import {IERC20} from "../src/interfaces/IERC20.sol";
import {IRiscZeroVerifier} from "../src/interfaces/IRiscZeroVerifier.sol";
import {MockUSDC} from "./mocks/MockUSDC.sol";
import {MockRiscZeroVerifier} from "./mocks/MockRiscZeroVerifier.sol";

/// @notice Shared fixture for CapabilityMarket tests.
abstract contract CapabilityMarketBase is Test {
    uint256 internal constant USDC_UNIT = 1e6; // 6 decimals

    CapabilityMarket internal market;
    MockUSDC internal usdc;
    MockRiscZeroVerifier internal verifier;

    address internal creator = makeAddr("creator");
    address internal alice = makeAddr("alice"); // YES staker
    address internal bob = makeAddr("bob"); // YES staker
    address internal carol = makeAddr("carol"); // NO staker
    address internal dave = makeAddr("dave"); // NO staker
    address internal miner = makeAddr("miner"); // committer / revealer
    address internal keeper = makeAddr("keeper");
    address internal frontrunner = makeAddr("frontrunner");

    // Default market parameters.
    uint256 internal t0 = 1_800_000_000; // fixture epoch
    bytes32 internal imageId = keccak256("image-id");
    bytes32 internal predicateArweaveTx = keccak256("predicate-arweave-tx");
    bytes32 internal marketParamsHash = keccak256("market-params-hash");
    uint256 internal lockWindowEnd;
    uint256 internal deadline;
    uint256 internal commitRevealWindow = 1 days;
    uint256 internal bountyBps = 100; // 1%

    // Default reveal payload.
    bytes32 internal solutionHash = keccak256("solution");
    bytes32 internal solutionArweaveTx = keccak256("solution-arweave-tx");
    bytes32 internal salt = keccak256("salt");

    function setUp() public virtual {
        vm.warp(t0);
        lockWindowEnd = t0 + 1 days;
        deadline = t0 + 2 days;

        usdc = new MockUSDC();
        verifier = new MockRiscZeroVerifier();
        market = new CapabilityMarket(IERC20(address(usdc)), IRiscZeroVerifier(address(verifier)));

        address[8] memory actors = [creator, alice, bob, carol, dave, miner, keeper, frontrunner];
        for (uint256 i = 0; i < actors.length; i++) {
            usdc.mint(actors[i], 1_000_000 * USDC_UNIT);
            vm.prank(actors[i]);
            usdc.approve(address(market), type(uint256).max);
        }
    }

    // -- helpers ---------------------------------------------------------

    function createDefaultMarket() internal returns (uint256) {
        return createMarketWithSeed(0);
    }

    function createMarketWithSeed(uint256 seedNoStake) internal returns (uint256) {
        vm.prank(creator);
        return market.createMarket(
            imageId,
            predicateArweaveTx,
            marketParamsHash,
            deadline,
            commitRevealWindow,
            lockWindowEnd,
            bountyBps,
            seedNoStake
        );
    }

    function stakeAs(address who, uint256 marketId, CapabilityMarket.Side side, uint256 amount) internal {
        vm.prank(who);
        market.stake(marketId, side, amount);
    }

    function commitmentFor(address revealer) internal view returns (bytes32) {
        return keccak256(abi.encodePacked(solutionHash, solutionArweaveTx, revealer, salt));
    }

    function commitAs(address who, uint256 marketId) internal {
        vm.prank(who);
        market.commit(marketId, commitmentFor(who));
    }

    function makeJournal(bytes32 imageId_, bytes32 paramsHash_, bytes32 submissionHash_, bool verdict)
        internal
        pure
        returns (bytes memory)
    {
        return abi.encode(
            CapabilityMarket.Journal({
                imageId: imageId_, marketParamsHash: paramsHash_, submissionHash: submissionHash_, verdict: verdict
            })
        );
    }

    function validJournal() internal view returns (bytes memory) {
        return makeJournal(imageId, marketParamsHash, solutionHash, true);
    }

    function sealFor(bytes memory journal) internal view returns (bytes memory) {
        return verifier.mockSeal(imageId, sha256(journal));
    }

    function revealAs(address who, uint256 marketId, bytes memory journal) internal {
        bytes memory seal = sealFor(journal); // precompute: an external call would eat the prank
        vm.prank(who);
        market.reveal(marketId, solutionHash, solutionArweaveTx, salt, seal, journal);
    }

    /// @dev expectRevert variant: the seal must be precomputed BEFORE vm.expectRevert, or the
    ///      mockSeal external call would absorb the expectation.
    function expectRevealRevert(bytes4 err, address who, uint256 marketId, bytes memory journal) internal {
        bytes memory seal = sealFor(journal);
        vm.expectRevert(err);
        vm.prank(who);
        market.reveal(marketId, solutionHash, solutionArweaveTx, salt, seal, journal);
    }

    /// @dev Full happy path up to (not including) reveal: stakes on both sides, warp into
    ///      commit window, miner commits.
    function setupStakedAndCommitted(uint256 marketId) internal {
        stakeAs(alice, marketId, CapabilityMarket.Side.YES, 100 * USDC_UNIT);
        stakeAs(bob, marketId, CapabilityMarket.Side.YES, 300 * USDC_UNIT);
        stakeAs(carol, marketId, CapabilityMarket.Side.NO, 200 * USDC_UNIT);
        stakeAs(dave, marketId, CapabilityMarket.Side.NO, 400 * USDC_UNIT);
        vm.warp(lockWindowEnd + 1);
        commitAs(miner, marketId);
    }
}
