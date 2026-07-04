// Prover-provider abstraction for the capability market (toon-meta#119 story 5,
// forked in toon-meta#84 "Prover market: Kalypso, optional").
//
// A miner proves a submission satisfies a market's predicate. TWO providers
// satisfy the SAME interface, so the reveal path never has to know which was
// used — the output is always {sealHex, journalHex} in exactly the form
// `CapabilityMarket.reveal(marketId, ..., bytes proof, bytes journal)` expects:
//
//   - LocalProver  — the DEFAULT, self-sufficient path. Shells out to the
//                    r0vm-backed `e2e-prover` host binary. A GPU is not required
//                    (r0vm proves on CPU), but Groth16 wrapping is slow.
//   - KalypsoProver — the OPTIONAL path (this story). A GPU-less miner outsources
//                    RISC Zero proof generation to Marlin's Kalypso marketplace,
//                    paying in USDC, and gets back the same risc0 seal. "Never a
//                    dependency": if Kalypso is unconfigured/unreachable, callers
//                    fall back to LocalProver.
//
// ProverProvider (structural interface):
//   async requestProof({ imageId, marketParams, submission, maxPriceUsdc, maxTimeSeconds })
//     -> { sealHex: "0x..", journalHex: "0x..", source: "local"|"kalypso", meta }
//
// The journal is DETERMINISTIC given (imageId, marketParams, submission, verdict),
// so both providers can hand back the identical 97-byte journal; only the seal
// differs by prover. See lib/journal.mjs.

import { spawn } from "node:child_process";
import { predicateJournal, encodeJournal } from "./journal.mjs";

const hex = (buf) => "0x" + Buffer.from(buf).toString("hex");
const strip0x = (s) => (s.startsWith("0x") ? s.slice(2) : s);

// ---------------------------------------------------------------------------
// LocalProver — delegate to the r0vm-backed e2e-prover host binary.
// ---------------------------------------------------------------------------

export class LocalProver {
  /**
   * @param {object} opts
   * @param {string} opts.predicatesDir  path to `predicates/` (the cargo workspace).
   * @param {string} opts.elfPath        path to the gunzipped canonical guest ELF.
   * @param {"dev"|"groth16"} [opts.mode] proving mode (default groth16 — the real seal).
   */
  constructor({ predicatesDir, elfPath, mode = "groth16" }) {
    this.predicatesDir = predicatesDir;
    this.elfPath = elfPath;
    this.mode = mode;
  }

  get name() { return "local"; }

  async requestProof({ rankBound = 49 } = {}) {
    // e2e-prover owns the canonical input manifest (image_id, market_params,
    // submission) and prints {journal_hex, seal_hex}. We do NOT re-implement
    // proving here — this is a thin delegation to the tool the repo already ships.
    const args = ["run", "-q", "-p", "e2e-prover", "--", "--elf", this.elfPath, "--mode", this.mode, "--rank-bound", String(rankBound)];
    const env = { ...process.env };
    if (this.mode === "dev") env.RISC0_DEV_MODE = "1";
    const out = await run("cargo", args, { cwd: this.predicatesDir, env });
    const json = JSON.parse(out.slice(out.indexOf("{")));
    return {
      sealHex: json.seal_hex,
      journalHex: json.journal_hex,
      source: "local",
      meta: { mode: this.mode, imageId: json.image_id, provingSeconds: json.proving_seconds },
    };
  }
}

// ---------------------------------------------------------------------------
// KalypsoProver — outsource RISC Zero proving to Marlin's Kalypso marketplace.
// ---------------------------------------------------------------------------
//
// Round-trip (per the kalypso-sdk `MarketPlace` API, v1.0.57):
//   1. Market setup (one-time, per image ID) — createPublicMarket(marketMetaData,
//      risc0Verifier, slashingPenalty, ivsPcrs). marketMetaData pins OUR image ID;
//      the verifier is a risc0 Groth16 verifier the network's provers honor.
//   2. approvePaymentTokenToMarketPlace(reward) in USDC (config.payment_token).
//   3. createAsk(marketId, proverData=submission, reward=maxPriceUsdc, expiry,
//      timeTakenForProofGeneration=maxTime, refundAddress, secretType, secretBuffer).
//      A staked prover is matched under the market's slashing-backed SLA.
//   4. getProofByAskId(askId, fromBlock) — reads the ProofCreated event; the
//      returned bytes ARE the risc0 seal. We pair it with the locally
//      reconstructed 97-byte journal (deterministic) for CapabilityMarket.reveal.
//
// USDC is the ONLY token the miner touches (reward + MARKET_CREATION_COST). POND /
// restaked collateral is the PROVER's concern (slashing bond), never the miner's.
//
// This provider LAZY-IMPORTS kalypso-sdk so the local path runs with zero install.
// It refuses to fabricate a proof: without live Kalypso config + a reachable
// market with a staked prover running our guest, it throws — it never returns a
// fake seal. See README §"Honest current state".

export class KalypsoProver {
  /**
   * @param {object} cfg
   * @param {string} cfg.rpcUrl              Kalypso chain RPC (e.g. Arbitrum Sepolia).
   * @param {string} cfg.privateKey          miner wallet key (USDC-funded).
   * @param {string} cfg.marketId            registered market id for our image ID.
   * @param {object} cfg.kalypsoConfig       KalspsoConfig (proof_market_place, entity_registry, payment_token=USDC, enclave endpoints…).
   * @param {(id:Buffer,mp:Buffer,sub:Buffer,verdict:boolean)=>object} [cfg.reconstructJournal]
   */
  constructor(cfg) {
    this.cfg = cfg;
  }

  get name() { return "kalypso"; }

  /** Config-completeness gate (does NOT prove the network is live — see reachability()). */
  static requiredConfig() {
    return ["rpcUrl", "privateKey", "marketId", "kalypsoConfig"];
  }

  isConfigured() {
    return KalypsoProver.requiredConfig().every((k) => this.cfg && this.cfg[k]);
  }

  async requestProof({ imageId, marketParams, submission, maxPriceUsdc, maxTimeSeconds }) {
    if (!this.isConfigured()) {
      throw new KalypsoUnavailable(
        "Kalypso is not configured. Set KALYPSO_RPC_URL, KALYPSO_PRIVATE_KEY, KALYPSO_MARKET_ID and a KalspsoConfig " +
        "(proof marketplace + enclave endpoints). Falling back to the local prover is the intended behavior — " +
        "Kalypso is a convenience, never a dependency."
      );
    }

    // Lazy import: only pulls the heavy SDK dep tree when the Kalypso path is used.
    let ethers, KalypsoSdk;
    try {
      ({ ethers } = await import("ethers"));
      ({ KalypsoSdk } = await import("kalypso-sdk"));
    } catch (e) {
      throw new KalypsoUnavailable(
        "kalypso-sdk / ethers not installed. Run `npm install` inside e2e/kalypso to enable the Kalypso path. (" + e.message + ")"
      );
    }

    const provider = new ethers.JsonRpcProvider(this.cfg.rpcUrl);
    const wallet = new ethers.Wallet(this.cfg.privateKey, provider);
    const kalypso = new KalypsoSdk(wallet, this.cfg.kalypsoConfig);
    const market = kalypso.MarketPlace();

    const fromBlock = await provider.getBlockNumber();

    // proverData is the public predicate input the market's prover runs against.
    // Our matmul predicate has NO private witness, so the submission travels as
    // public prover data (secretBuffer stays empty; the market is a NO_ENCLAVE market).
    const submissionBytes = "0x" + Buffer.from(submission).toString("hex");
    const askTx = await market.createAsk(
      this.cfg.marketId,
      submissionBytes,
      maxPriceUsdc,                 // reward, in USDC (config.payment_token)
      Math.floor(Date.now() / 1000) + (maxTimeSeconds ?? 3600), // assignment expiry
      maxTimeSeconds ?? 3600,       // timeTakenForProofGeneration budget
      await wallet.getAddress(),    // refund address on non-fulfilment (SLA slash → refund)
      1,                            // secretType: 1 == calldata / public inputs
      Buffer.alloc(0),              // no private secret
      false,
    );
    const receipt = await askTx.wait();
    const askId = await market.getAskId(receipt);

    // Poll the marketplace until a staked prover fulfils the ask (or SLA deadline).
    const sealHex = await pollProof(market, askId, fromBlock, maxTimeSeconds ?? 3600);

    // Reconstruct the deterministic 97-byte journal to pair with the outsourced seal.
    const j = predicateJournal(Buffer.from(strip0x(imageId), "hex"), Buffer.from(marketParams), Buffer.from(submission), true);
    return {
      sealHex,
      journalHex: hex(encodeJournal(j)),
      source: "kalypso",
      meta: { marketId: this.cfg.marketId, askId: String(askId) },
    };
  }

  /** Best-effort network preflight: RPC reachable + marketplace contract present. */
  async reachability() {
    if (!this.isConfigured()) return { reachable: false, reason: "unconfigured" };
    try {
      const { ethers } = await import("ethers");
      const provider = new ethers.JsonRpcProvider(this.cfg.rpcUrl);
      const [chainId, code] = await Promise.all([
        provider.getNetwork().then((n) => Number(n.chainId)),
        provider.getCode(this.cfg.kalypsoConfig.proof_market_place),
      ]);
      const hasContract = code && code !== "0x";
      return { reachable: hasContract, chainId, proofMarketplaceDeployed: hasContract };
    } catch (e) {
      return { reachable: false, reason: e.message };
    }
  }
}

export class KalypsoUnavailable extends Error {
  constructor(msg) { super(msg); this.name = "KalypsoUnavailable"; }
}

async function pollProof(market, askId, fromBlock, timeoutSeconds) {
  const deadline = Date.now() + timeoutSeconds * 1000;
  while (Date.now() < deadline) {
    const res = await market.getProofByAskId(String(askId), fromBlock);
    if (res.proof_generated) return typeof res.proof === "string" ? res.proof : hex(res.proof);
    if (res.message && res.message.includes("InvalidInputs")) {
      throw new Error(`Kalypso prover rejected inputs for ask ${askId} (InvalidInputsDetected)`);
    }
    await sleep(5000);
  }
  throw new Error(`Kalypso ask ${askId} not fulfilled within ${timeoutSeconds}s (SLA deadline crossed)`);
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

function run(cmd, args, { cwd, env } = {}) {
  return new Promise((resolve, reject) => {
    const p = spawn(cmd, args, { cwd, env, stdio: ["ignore", "pipe", "inherit"] });
    let out = "";
    p.stdout.on("data", (d) => (out += d.toString()));
    p.on("error", reject);
    p.on("close", (code) => (code === 0 ? resolve(out) : reject(new Error(`${cmd} exited ${code}`))));
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
