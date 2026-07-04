#!/usr/bin/env node
// Upload a predicate guest ELF to Arweave via the Turbo free tier (toon-meta#112).
//
// Gzips the ELF (single-member RFC 1952 stream) and uploads it with
// Content-Type: application/gzip. Size-optimized guest ELFs (~164 KB) gzip to
// well under the ~100 KiB Turbo free-upload threshold, so no funded wallet is
// needed — a throwaway Arweave JWK is generated (or loaded) purely for signing.
// See docs/predicate-envelope.md §3.1 check 2 (toon-protocol/toon-meta):
// the image ID always commits to the DECOMPRESSED ELF; gzip is pure transport.
//
// Usage:
//   node e2e/upload-predicate.mjs <path-to-guest-elf> [--jwk <jwk.json>]
//   node e2e/upload-predicate.mjs <path> --raw [--content-type <ct>] [--jwk <jwk.json>]
//
// `--raw` uploads the file VERBATIM (no gzip) — used for the canonical input
// manifest bytes, whose sha256 is the market's marketParamsHash, so a fetcher
// must get the exact bytes back (envelope spec §2.3 / §3.3 step 3). Default
// content type for --raw is application/octet-stream.
//
// Prints the Arweave tx id on success. Keep the JWK out of git — by default a
// fresh throwaway key is generated per run (the tx id, not the key, is what
// gets pinned on-chain).

import { readFile, writeFile } from "node:fs/promises";
import { gzipSync, constants as zlibConstants } from "node:zlib";
import { TurboFactory } from "@ardrive/turbo-sdk";
import Arweave from "arweave";
import { Readable } from "node:stream";

const FREE_TIER_BYTES = 100 * 1024; // Turbo uploads under ~100 KiB are free

async function main() {
  const args = process.argv.slice(2);
  const jwkFlag = args.indexOf("--jwk");
  let jwkPath = null;
  if (jwkFlag !== -1) {
    jwkPath = args[jwkFlag + 1];
    args.splice(jwkFlag, 2);
  }
  const rawMode = args.includes("--raw");
  if (rawMode) args.splice(args.indexOf("--raw"), 1);
  let contentType = null;
  const ctFlag = args.indexOf("--content-type");
  if (ctFlag !== -1) {
    contentType = args[ctFlag + 1];
    args.splice(ctFlag, 2);
  }
  const type = rawMode ? "input-manifest" : "predicate-guest-elf";
  const elfPath = args[0];
  if (!elfPath) {
    console.error("usage: node e2e/upload-predicate.mjs <path> [--raw] [--content-type <ct>] [--jwk <jwk.json>]");
    process.exit(2);
  }

  const raw = await readFile(elfPath);
  let payload, uploadContentType;
  if (rawMode) {
    // Verbatim upload — the pinned bytes must equal the exact input (the
    // manifest bytes hashed into marketParamsHash).
    payload = raw;
    uploadContentType = contentType || "application/octet-stream";
    console.error(`raw ${raw.length} bytes (verbatim, ${uploadContentType})`);
  } else {
    // If the input is already a gzip stream (magic 1f 8b), upload it verbatim
    // so the bytes pinned on Arweave are exactly the committed canonical
    // artifact.
    const alreadyGzip = raw.length > 2 && raw[0] === 0x1f && raw[1] === 0x8b;
    payload = alreadyGzip
      ? raw
      : gzipSync(raw, { level: zlibConstants.Z_BEST_COMPRESSION });
    uploadContentType = contentType || "application/gzip";
    console.error(
      `elf ${raw.length} bytes -> gzip ${payload.length} bytes` +
        (alreadyGzip ? " (input was already gzip; uploading verbatim)" : "")
    );
  }
  const gz = payload;
  if (gz.length >= FREE_TIER_BYTES) {
    console.error(
      `WARNING: ${gz.length} bytes >= ${FREE_TIER_BYTES}; upload may require Turbo credits`
    );
  }

  let jwk;
  if (jwkPath) {
    jwk = JSON.parse(await readFile(jwkPath, "utf8"));
  } else {
    // Throwaway signing key: free-tier uploads need a signer, not a balance.
    const arweave = Arweave.init({});
    jwk = await arweave.wallets.generate();
    if (process.env.SAVE_JWK) {
      await writeFile(process.env.SAVE_JWK, JSON.stringify(jwk));
      console.error(`throwaway JWK saved to ${process.env.SAVE_JWK} (do NOT commit)`);
    }
  }

  const turbo = TurboFactory.authenticated({ privateKey: jwk });
  const result = await turbo.uploadFile({
    fileStreamFactory: () => Readable.from(gz),
    fileSizeFactory: () => gz.length,
    dataItemOpts: {
      tags: [
        { name: "Content-Type", value: uploadContentType },
        { name: "App-Name", value: "toon-capability-market" },
        { name: "Type", value: type },
      ],
    },
  });

  console.error(`uploaded: https://arweave.net/${result.id}`);
  console.log(result.id);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
