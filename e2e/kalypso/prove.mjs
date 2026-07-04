#!/usr/bin/env node
// Kalypso optional prover CLI (toon-meta#119 story 5 / #84 "Prover market: Kalypso, optional").
//
// Produces a {sealHex, journalHex} pair for the flagship matmul market's rank-49
// witness, in EXACTLY the form `CapabilityMarket.reveal(marketId, ..., bytes proof,
// bytes journal)` consumes. Two sources, one output shape:
//
//   --source local    (default) prove locally via the r0vm-backed e2e-prover.
//   --source kalypso  outsource proving to Marlin's Kalypso marketplace (USDC).
//   --source auto     try Kalypso if configured+reachable, else fall back to local.
//
// Usage:
//   node prove.mjs --source local   --mode dev|groth16 [--elf <path>]
//   node prove.mjs --source kalypso --max-price-usdc 500000 --max-time 1800
//   node prove.mjs --source auto    --max-price-usdc 500000 --mode groth16
//
// Kalypso config comes from env (see .env.kalypso.example): KALYPSO_RPC_URL,
// KALYPSO_PRIVATE_KEY, KALYPSO_MARKET_ID, KALYPSO_PROOF_MARKETPLACE,
// KALYPSO_ENTITY_REGISTRY, KALYPSO_PAYMENT_TOKEN (USDC), KALYPSO_MATCHING_ENGINE_URL, …

import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { existsSync } from "node:fs";
import { KNOWN_WITNESS } from "./lib/matmul.mjs";
import { LocalProver, KalypsoProver, KalypsoUnavailable } from "./lib/providers.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..");            // e2e/kalypso -> repo root
const PREDICATES_DIR = resolve(REPO, "predicates");
const IMAGE_ID = "660d47e33136b07e362d5efac8669ee3d31603df5aa5c12dba41f91156e8ecff"; // predicates/ARTIFACTS.json

function parseArgs(argv) {
  const a = { source: "local", mode: "groth16", maxPriceUsdc: "500000", maxTime: 1800, rankBound: 49, elf: null };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    const v = () => argv[++i];
    if (k === "--source") a.source = v();
    else if (k === "--mode") a.mode = v();
    else if (k === "--max-price-usdc") a.maxPriceUsdc = v();
    else if (k === "--max-time") a.maxTime = Number(v());
    else if (k === "--rank-bound") a.rankBound = Number(v());
    else if (k === "--elf") a.elf = v();
    else if (k === "-h" || k === "--help") a.help = true;
    else throw new Error(`unknown arg: ${k}`);
  }
  return a;
}

function kalypsoConfigFromEnv() {
  const e = process.env;
  if (!e.KALYPSO_RPC_URL || !e.KALYPSO_PRIVATE_KEY || !e.KALYPSO_MARKET_ID || !e.KALYPSO_PROOF_MARKETPLACE) {
    return null;
  }
  return {
    rpcUrl: e.KALYPSO_RPC_URL,
    privateKey: e.KALYPSO_PRIVATE_KEY,
    marketId: e.KALYPSO_MARKET_ID,
    kalypsoConfig: {
      payment_token: e.KALYPSO_PAYMENT_TOKEN,           // USDC — the miner's only token
      staking_token: e.KALYPSO_STAKING_TOKEN,           // POND — prover-side only
      generator_registry: e.KALYPSO_GENERATOR_REGISTRY,
      attestation_verifier: e.KALYPSO_ATTESTATION_VERIFIER,
      entity_registry: e.KALYPSO_ENTITY_REGISTRY,
      proof_market_place: e.KALYPSO_PROOF_MARKETPLACE,
      tee_verifier_deployer: e.KALYPSO_TEE_VERIFIER_DEPLOYER,
      checkInputUrl: e.KALYPSO_CHECK_INPUT_URL,
      attestationVerifierEndPoint: e.KALYPSO_ATTESTATION_ENDPOINT,
      matchingEngineEnclave: e.KALYPSO_MATCHING_ENGINE_URL
        ? { url: e.KALYPSO_MATCHING_ENGINE_URL, utilityUrl: e.KALYPSO_MATCHING_ENGINE_UTILITY_URL || e.KALYPSO_MATCHING_ENGINE_URL }
        : undefined,
    },
  };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    console.log("Usage: node prove.mjs --source local|kalypso|auto [--mode dev|groth16] [--max-price-usdc N] [--max-time S]");
    return;
  }

  const elfPath = args.elf || resolve(HERE, "matmul-guest.elf");
  const req = {
    imageId: IMAGE_ID,
    marketParams: KNOWN_WITNESS.marketParams,
    submission: KNOWN_WITNESS.submission,
    maxPriceUsdc: args.maxPriceUsdc,
    maxTimeSeconds: args.maxTime,
    rankBound: args.rankBound,
  };

  const local = new LocalProver({ predicatesDir: PREDICATES_DIR, elfPath, mode: args.mode });
  const kcfg = kalypsoConfigFromEnv();
  const kalypso = kcfg ? new KalypsoProver(kcfg) : null;

  let result;
  if (args.source === "local") {
    ensureElf(elfPath);
    result = await local.requestProof(req);
  } else if (args.source === "kalypso") {
    if (!kalypso) throw new KalypsoUnavailable("Kalypso not configured — see .env.kalypso.example.");
    const reach = await kalypso.reachability();
    console.error("[kalypso] reachability:", JSON.stringify(reach));
    if (!reach.reachable) throw new KalypsoUnavailable(`Kalypso unreachable: ${JSON.stringify(reach)}`);
    result = await kalypso.requestProof(req);
  } else if (args.source === "auto") {
    if (kalypso) {
      const reach = await kalypso.reachability();
      console.error("[kalypso] reachability:", JSON.stringify(reach));
      if (reach.reachable) {
        try {
          result = await kalypso.requestProof(req);
        } catch (e) {
          console.error(`[kalypso] failed (${e.message}); falling back to local prover.`);
        }
      } else {
        console.error("[kalypso] unreachable; falling back to local prover.");
      }
    } else {
      console.error("[kalypso] unconfigured; using local prover.");
    }
    if (!result) {
      ensureElf(elfPath);
      result = await local.requestProof(req);
    }
  } else {
    throw new Error(`unknown --source ${args.source}`);
  }

  console.log(JSON.stringify({
    image_id: "0x" + IMAGE_ID,
    rank_bound: args.rankBound,
    source: result.source,
    seal_hex: result.sealHex,
    journal_hex: result.journalHex,
    reveal_hint: "cast send <market> 'reveal(uint256,bytes32,bytes,bytes)' <marketId> <salt> <seal_hex> <journal_hex>",
    meta: result.meta,
  }, null, 2));
}

function ensureElf(elfPath) {
  if (!existsSync(elfPath)) {
    throw new Error(
      `guest ELF not found at ${elfPath}. Gunzip the canonical artifact:\n` +
      `  gzip -dc predicates/artifacts/matmul-guest.canonical.bin.gz > ${elfPath}`
    );
  }
}

main().catch((e) => {
  console.error("error:", e.message);
  process.exit(1);
});
