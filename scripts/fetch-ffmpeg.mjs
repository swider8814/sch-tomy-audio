#!/usr/bin/env node
// Downloads the pinned static ffmpeg/ffprobe builds for a Rust target triple,
// verifies their SHA-256 and installs them as Tauri sidecars:
//   src-tauri/binaries/ffmpeg-<triple>[.exe], ffprobe-<triple>[.exe],
//   src-tauri/binaries/FFMPEG_LICENSE.txt, FFMPEG_README.txt
//
// Usage: node scripts/fetch-ffmpeg.mjs [--target <triple>] [--from-path]
//   --from-path  copy ffmpeg/ffprobe found on PATH instead (development on
//                platforms without a pinned build, e.g. Linux).
// Only Node built-ins are used (no dependencies).

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, delimiter } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync } from "node:zlib";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "src-tauri", "binaries");
const sources = JSON.parse(readFileSync(join(root, "scripts", "ffmpeg-sources.json"), "utf8"));

function arg(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

function hostTriple() {
  const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  const line = out.split(/\r?\n/).find((l) => l.startsWith("host:"));
  if (!line) throw new Error("cannot determine host triple from rustc -vV");
  return line.slice(5).trim();
}

const target = arg("--target") ?? hostTriple();
const exe = target.includes("windows") ? ".exe" : "";
mkdirSync(outDir, { recursive: true });

async function download(url, attempts = 4) {
  for (let i = 1; ; i++) {
    try {
      const res = await fetch(url, { redirect: "follow" });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      return Buffer.from(await res.arrayBuffer());
    } catch (e) {
      if (i >= attempts) throw new Error(`download failed: ${url}: ${e.message}`);
      await new Promise((r) => setTimeout(r, 2000 * i));
    }
  }
}

function sha256(buf) {
  return createHash("sha256").update(buf).digest("hex");
}

async function fetchVerified(entry) {
  const buf = await download(entry.url);
  const got = sha256(buf);
  if (got !== entry.sha256) {
    throw new Error(`checksum mismatch for ${entry.url}\n  expected ${entry.sha256}\n  got      ${got}`);
  }
  return buf;
}

function findOnPath(name) {
  for (const dir of (process.env.PATH ?? "").split(delimiter)) {
    const p = join(dir, name + exe);
    if (dir && existsSync(p)) return p;
  }
  throw new Error(`${name} not found on PATH`);
}

if (process.argv.includes("--from-path")) {
  for (const tool of ["ffmpeg", "ffprobe"]) {
    const dest = join(outDir, `${tool}-${target}${exe}`);
    copyFileSync(findOnPath(tool), dest);
    chmodSync(dest, 0o755);
    console.log(`copied ${tool} from PATH -> ${dest}`);
  }
  writeFileSync(join(outDir, "FFMPEG_LICENSE.txt"), "Development build: ffmpeg copied from the system PATH.\n");
  writeFileSync(join(outDir, "FFMPEG_README.txt"), "Development build.\n");
  process.exit(0);
}

const t = sources.targets[target];
if (!t) {
  console.error(`No pinned ffmpeg build for ${target}. Supported: ${Object.keys(sources.targets).join(", ")}.`);
  console.error("For development use --from-path.");
  process.exit(1);
}

for (const tool of ["ffmpeg", "ffprobe"]) {
  const gz = await fetchVerified(t[tool]);
  const dest = join(outDir, `${tool}-${target}${exe}`);
  writeFileSync(dest, gunzipSync(gz));
  chmodSync(dest, 0o755);
  console.log(`${tool}: ${t[tool].url} (sha256 ok) -> ${dest}`);
}
writeFileSync(join(outDir, "FFMPEG_LICENSE.txt"), await fetchVerified(t.license));
writeFileSync(join(outDir, "FFMPEG_README.txt"), await fetchVerified(t.readme));
console.log("license and readme verified");
