// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CommonBase} from "forge-std/Base.sol";
import {StdCheats} from "forge-std/StdCheats.sol";
import {StdUtils} from "forge-std/StdUtils.sol";
import {CapabilityMarket} from "../src/CapabilityMarket.sol";
import {IERC20} from "../src/interfaces/IERC20.sol";
import {IRiscZeroVerifier} from "../src/interfaces/IRiscZeroVerifier.sol";
import {MockUSDC} from "./mocks/MockUSDC.sol";
import {MockRiscZeroVerifier} from "./mocks/MockRiscZeroVerifier.sol";

/// Random walk over the whole lifecycle (create/stake/warp/commit/reveal/settle/withdraw)
/// across several concurrent markets and actors.
contract Handler is CommonBase, StdCheats, StdUtils {
    CapabilityMarket public market;
    MockUSDC public usdc;
    MockRiscZeroVerifier public verifier;

    address[] public actors;

    // ghost accounting
    uint256 public ghostStakedIn; // USDC that entered the contract
    uint256 public ghostPaidOut; // USDC that left (withdrawals + bounties)

    bytes32 internal constant SOLUTION_HASH = keccak256("solution");
    bytes32 internal constant SOLUTION_ARWEAVE_TX = keccak256("solution-arweave-tx");
    bytes32 internal constant SALT = keccak256("salt");
    bytes32 internal constant IMAGE_ID = keccak256("image-id");
    bytes32 internal constant PARAMS_HASH = keccak256("market-params-hash");

    constructor(CapabilityMarket market_, MockUSDC usdc_, MockRiscZeroVerifier verifier_) {
        market = market_;
        usdc = usdc_;
        verifier = verifier_;

        for (uint256 i = 0; i < 6; i++) {
            address a = makeAddr(string(abi.encodePacked("h-actor", i)));
            actors.push(a);
            usdc.mint(a, type(uint96).max);
            vm.prank(a);
            usdc.approve(address(market), type(uint256).max);
        }
    }

    function _actor(uint256 seed) internal view returns (address) {
        return actors[seed % actors.length];
    }

    function _market(uint256 seed) internal view returns (uint256, CapabilityMarket.Market memory) {
        uint256 count = market.marketCount();
        if (count == 0) {
            CapabilityMarket.Market memory empty;
            return (type(uint256).max, empty);
        }
        uint256 id = seed % count;
        return (id, market.getMarket(id));
    }

    // -- ops ---------------------------------------------------------------

    function createMarket(uint256 actorSeed, uint256 lockDelta, uint256 dlDelta, uint256 crw, uint256 bps, uint256 seed)
        external
    {
        if (market.marketCount() >= 5) return; // keep the walk dense
        address a = _actor(actorSeed);
        lockDelta = bound(lockDelta, 1 hours, 3 days);
        dlDelta = bound(dlDelta, lockDelta + 1, lockDelta + 3 days);
        crw = bound(crw, 1 hours, 2 days);
        bps = bound(bps, 0, market.MAX_BOUNTY_BPS());
        seed = bound(seed, 0, 1000e6);

        vm.prank(a);
        market.createMarket(
            IMAGE_ID,
            keccak256("predicate"),
            PARAMS_HASH,
            block.timestamp + dlDelta,
            crw,
            block.timestamp + lockDelta,
            bps,
            seed
        );
        ghostStakedIn += seed;
    }

    function stake(uint256 actorSeed, uint256 marketSeed, bool yes, uint256 amount) external {
        (uint256 id, CapabilityMarket.Market memory m) = _market(marketSeed);
        if (id == type(uint256).max) return;
        if (block.timestamp > m.lockWindowEnd) return;

        address a = _actor(actorSeed);
        CapabilityMarket.Stake memory s = market.getStake(id, a);
        CapabilityMarket.Side side = yes ? CapabilityMarket.Side.YES : CapabilityMarket.Side.NO;
        if (s.amount > 0) side = s.side; // never attempt a side switch
        amount = bound(amount, 1, 10_000e6);

        vm.prank(a);
        market.stake(id, side, amount);
        ghostStakedIn += amount;
    }

    function commit(uint256 actorSeed, uint256 marketSeed) external {
        (uint256 id, CapabilityMarket.Market memory m) = _market(marketSeed);
        if (id == type(uint256).max) return;
        if (block.timestamp <= m.lockWindowEnd || block.timestamp > m.deadline) return;

        address a = _actor(actorSeed);
        vm.prank(a);
        market.commit(id, keccak256(abi.encodePacked(SOLUTION_HASH, SOLUTION_ARWEAVE_TX, a, SALT)));
    }

    function reveal(uint256 actorSeed, uint256 marketSeed) external {
        (uint256 id, CapabilityMarket.Market memory m) = _market(marketSeed);
        if (id == type(uint256).max) return;
        if (m.resolution != CapabilityMarket.Resolution.Unresolved) return;
        if (block.timestamp > m.deadline + m.commitRevealWindow) return;

        address a = _actor(actorSeed);
        if (market.getCommitment(id, a).commitmentHash == bytes32(0)) return;

        bytes memory journal = abi.encode(
            CapabilityMarket.Journal({
                imageId: IMAGE_ID, marketParamsHash: PARAMS_HASH, submissionHash: SOLUTION_HASH, verdict: true
            })
        );
        bytes memory seal = verifier.mockSeal(IMAGE_ID, sha256(journal));
        vm.prank(a);
        market.reveal(id, SOLUTION_HASH, SOLUTION_ARWEAVE_TX, SALT, seal, journal);
    }

    function settleTimeout(uint256 actorSeed, uint256 marketSeed) external {
        (uint256 id, CapabilityMarket.Market memory m) = _market(marketSeed);
        if (id == type(uint256).max) return;
        if (m.resolution != CapabilityMarket.Resolution.Unresolved) return;
        if (block.timestamp <= m.deadline + m.commitRevealWindow) return;

        address a = _actor(actorSeed);
        uint256 before = usdc.balanceOf(a);
        vm.prank(a);
        market.settleTimeout(id);
        ghostPaidOut += usdc.balanceOf(a) - before;
    }

    function withdraw(uint256 actorSeed, uint256 marketSeed) external {
        (uint256 id,) = _market(marketSeed);
        if (id == type(uint256).max) return;

        address a = _actor(actorSeed);
        uint256 entitled = market.getWithdrawable(id, a);
        if (entitled == 0) return;

        vm.prank(a);
        market.withdraw(id);
        ghostPaidOut += entitled;
    }

    function warp(uint256 delta) external {
        delta = bound(delta, 10 minutes, 2 days);
        vm.warp(block.timestamp + delta);
    }

    // -- views for the invariant --------------------------------------------

    function actorCount() external view returns (uint256) {
        return actors.length;
    }

    function sumEntitlements() external view returns (uint256 sum) {
        uint256 count = market.marketCount();
        for (uint256 id = 0; id < count; id++) {
            for (uint256 i = 0; i < actors.length; i++) {
                sum += market.getWithdrawable(id, actors[i]);
            }
        }
    }
}

contract CapabilityMarketInvariantTest is Test {
    CapabilityMarket internal market;
    MockUSDC internal usdc;
    MockRiscZeroVerifier internal verifier;
    Handler internal handler;

    function setUp() public {
        vm.warp(1_800_000_000);
        usdc = new MockUSDC();
        verifier = new MockRiscZeroVerifier();
        market = new CapabilityMarket(IERC20(address(usdc)), IRiscZeroVerifier(address(verifier)));
        handler = new Handler(market, usdc, verifier);

        targetContract(address(handler));
        bytes4[] memory selectors = new bytes4[](7);
        selectors[0] = Handler.createMarket.selector;
        selectors[1] = Handler.stake.selector;
        selectors[2] = Handler.commit.selector;
        selectors[3] = Handler.reveal.selector;
        selectors[4] = Handler.settleTimeout.selector;
        selectors[5] = Handler.withdraw.selector;
        selectors[6] = Handler.warp.selector;
        targetSelector(FuzzSelector({addr: address(handler), selectors: selectors}));
    }

    /// The core solvency invariant: the contract always holds at least the sum of every
    /// staker's unwithdrawn entitlement, across all markets.
    function invariant_solvency_balanceCoversEntitlements() public view {
        assertGe(usdc.balanceOf(address(market)), handler.sumEntitlements());
    }

    /// Ghost accounting: contract balance is exactly what came in minus what went out.
    function invariant_conservation_exactBalance() public view {
        assertEq(usdc.balanceOf(address(market)), handler.ghostStakedIn() - handler.ghostPaidOut());
    }
}
