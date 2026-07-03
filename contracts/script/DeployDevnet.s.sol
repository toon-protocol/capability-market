// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Script, console2} from "forge-std/Script.sol";

import {CapabilityMarket} from "../src/CapabilityMarket.sol";
import {IERC20} from "../src/interfaces/IERC20.sol";
import {IRiscZeroVerifier} from "../src/interfaces/IRiscZeroVerifier.sol";

import {RiscZeroGroth16Verifier} from "risc0/groth16/RiscZeroGroth16Verifier.sol";
import {ControlID} from "risc0/groth16/ControlID.sol";
import {RiscZeroMockVerifier} from "risc0/test/RiscZeroMockVerifier.sol";

/// @title DeployDevnet — TOON devnet deployment for the capability market
/// @notice Deploys:
///           1. RiscZeroGroth16Verifier — the REAL verifier, pinned to the zkVM 3.0.x
///              control root via the vendored risc0-ethereum v3.0.1 ControlID constants.
///           2. RiscZeroMockVerifier(0xFFFFFFFF) — accepts risc0 dev-mode (fake) seals,
///              selector matches risc0's FakeReceipt selector.
///           3. CapabilityMarket wired to the real verifier ("marketReal").
///           4. CapabilityMarket wired to the mock verifier ("marketMock").
///         Both markets stake the same USDC token.
///
/// Env:
///   DEPLOYER_KEY — deployer private key (hex, 0x-prefixed)
///   USDC_ADDRESS — staking token (defaults to the TOON devnet USDC)
///
/// Usage (RPC parameterized on the CLI):
///   set -a; source ../e2e/.env.devnet; set +a
///   forge script script/DeployDevnet.s.sol --rpc-url "$DEVNET_RPC_URL" --broadcast
contract DeployDevnet is Script {
    /// @dev TOON devnet USDC (anvil chainId 31337, 6 decimals).
    address internal constant DEFAULT_DEVNET_USDC = 0x5FbDB2315678afecb367f032d93F642f64180aa3;

    /// @dev risc0 dev-mode fake receipts are sealed with the FakeReceipt selector.
    bytes4 internal constant DEV_MODE_SELECTOR = bytes4(0xFFFFFFFF);

    function run() external {
        uint256 deployerKey = vm.envUint("DEPLOYER_KEY");
        address usdc = vm.envOr("USDC_ADDRESS", DEFAULT_DEVNET_USDC);

        vm.startBroadcast(deployerKey);

        RiscZeroGroth16Verifier groth16Verifier =
            new RiscZeroGroth16Verifier(ControlID.CONTROL_ROOT, ControlID.BN254_CONTROL_ID);
        RiscZeroMockVerifier mockVerifier = new RiscZeroMockVerifier(DEV_MODE_SELECTOR);

        CapabilityMarket marketReal =
            new CapabilityMarket(IERC20(usdc), IRiscZeroVerifier(address(groth16Verifier)));
        CapabilityMarket marketMock =
            new CapabilityMarket(IERC20(usdc), IRiscZeroVerifier(address(mockVerifier)));

        vm.stopBroadcast();

        console2.log("chainId:          ", block.chainid);
        console2.log("usdc:             ", usdc);
        console2.log("groth16Verifier:  ", address(groth16Verifier));
        console2.log("groth16 selector: ");
        console2.logBytes4(groth16Verifier.SELECTOR());
        console2.log("control root:     ");
        console2.logBytes32(ControlID.CONTROL_ROOT);
        console2.log("bn254 control id: ");
        console2.logBytes32(ControlID.BN254_CONTROL_ID);
        console2.log("mockVerifier:     ", address(mockVerifier));
        console2.log("marketReal:       ", address(marketReal));
        console2.log("marketMock:       ", address(marketMock));
    }
}
