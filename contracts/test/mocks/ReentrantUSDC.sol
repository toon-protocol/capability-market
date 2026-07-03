// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {CapabilityMarket} from "../../src/CapabilityMarket.sol";

/// @notice A malicious 6-decimal ERC20 that attempts to reenter CapabilityMarket.withdraw on
///         every outbound transfer. Used to prove the nonReentrant guard + checks-effects-
///         interactions ordering: the reentrant attempt MUST revert, and the honest caller
///         must still be paid exactly once (no double-withdraw).
contract ReentrantUSDC {
    uint8 public constant decimals = 6;

    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    CapabilityMarket public target;
    uint256 public attackMarketId;
    bool public armed;
    uint256 public reentryAttempts;
    uint256 public reentryReverts;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function arm(CapabilityMarket target_, uint256 marketId) external {
        target = target_;
        attackMarketId = marketId;
        armed = true;
    }

    function disarm() external {
        armed = false;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        _reenter();
        return _move(msg.sender, to, amount);
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            require(allowed >= amount, "allowance");
            allowance[from][msg.sender] = allowed - amount;
        }
        return _move(from, to, amount);
    }

    function _move(address from, address to, uint256 amount) internal returns (bool) {
        require(balanceOf[from] >= amount, "balance");
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        return true;
    }

    /// @dev Attempt a reentrant withdraw during an outbound transfer. Swallow the revert so
    ///      the outer (honest) withdraw is allowed to complete — the test then asserts the
    ///      attacker was paid exactly once.
    function _reenter() internal {
        if (!armed) return;
        armed = false; // one shot; avoid infinite recursion if the guard were absent
        reentryAttempts++;
        try target.withdraw(attackMarketId) {
            // reached only if the guard failed to stop reentry
        } catch {
            reentryReverts++;
        }
    }
}
