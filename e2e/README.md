# e2e — TOON devnet deployment

Live deployment record: [`../deployments/devnet.json`](../deployments/devnet.json).

## Devnet facts

- RPC: `https://evm-rpc.devnet.toonprotocol.dev` (anvil, chainId 31337)
- USDC: `0x5FbDB2315678afecb367f032d93F642f64180aa3` (6 decimals)
- Faucet (100 ETH + 10,000 USDC per call):

  ```sh
  curl -X POST https://faucet.devnet.toonprotocol.dev/api/request \
    -H 'content-type: application/json' -d '{"address":"0xYOURADDR"}'
  ```

## Shared-anvil rule: NO time warping

This anvil instance is **shared devnet infrastructure** backing live TOON payment
channels. Never call `evm_increaseTime`, `evm_mine`, `evm_setNextBlockTimestamp`,
or any `anvil_*` cheatcode against it. To exercise time-windowed market states
(lock/commit/reveal), create markets with short real-time windows and wait, or run
a local anvil.

## Deploy procedure

1. Generate a deployer key and fund it:

   ```sh
   cast wallet new
   curl -X POST https://faucet.devnet.toonprotocol.dev/api/request \
     -H 'content-type: application/json' -d '{"address":"0xDEPLOYER"}'
   ```

2. Configure secrets (never committed):

   ```sh
   cp e2e/.env.devnet.example e2e/.env.devnet   # then fill in DEPLOYER_KEY
   ```

3. Deploy (from `contracts/`):

   ```sh
   set -a; source ../e2e/.env.devnet; set +a
   forge script script/DeployDevnet.s.sol --rpc-url "$DEVNET_RPC_URL" --broadcast
   ```

   This deploys `RiscZeroGroth16Verifier` (real, zkVM 3.0.x control root),
   `RiscZeroMockVerifier(0xffffffff)` (accepts risc0 dev-mode fake seals), and two
   `CapabilityMarket` instances — one per verifier — both staking devnet USDC.

4. Update `deployments/devnet.json` with the new addresses, deploy blocks (from
   `contracts/broadcast/DeployDevnet.s.sol/31337/run-latest.json` — confirm the
   per-address tx hash/block with `cast receipt`, the broadcast file can pair
   same-block hashes out of order), selector, and control root.

5. Smoke-check:

   ```sh
   cast call $MARKET "usdc()(address)" --rpc-url "$DEVNET_RPC_URL"
   cast call $MARKET "verifier()(address)" --rpc-url "$DEVNET_RPC_URL"
   ```

The RISC Zero verifier contracts are vendored (no submodules) at
`contracts/lib/risc0-ethereum/` — see `VENDORED.md` there for version/upgrade notes.
