#!/usr/bin/env node
// Downloads the pinned sidecars for a Rust target triple, verifies their
// SHA-256 and installs them for Tauri (externalBin):
//   src-tauri/binaries/{ffmpeg,ffprobe,yt-dlp,deno}-<triple>[.exe]
//   src-tauri/binaries/{FFMPEG_LICENSE,FFMPEG_README,YTDLP_LICENSE,DENO_LICENSE}.txt
// (yt-dlp and Deno are used for the YouTube download, see src/youtube.rs).
//
// Usage: node scripts/fetch-ffmpeg.mjs [--target <triple>] [--from-path]
//   --from-path  copy ffmpeg/ffprobe (and yt-dlp/deno if present) found on
//                PATH instead (development on platforms without a pinned
//                build, e.g. Linux); missing yt-dlp/deno become stubs.
// Only Node built-ins are used (no dependencies).

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, delimiter } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync, inflateRawSync } from "node:zlib";

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

/** Extracts one member from a zip archive (stored or deflated). */
function unzipMember(zip, member) {
  // End of central directory record: signature 0x06054b50, search backwards.
  let eocd = -1;
  for (let i = zip.length - 22; i >= Math.max(0, zip.length - 65557); i--) {
    if (zip.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw new Error("not a zip file");
  const count = zip.readUInt16LE(eocd + 10);
  let p = zip.readUInt32LE(eocd + 16);
  for (let n = 0; n < count; n++) {
    if (zip.readUInt32LE(p) !== 0x02014b50) throw new Error("bad zip central directory");
    const method = zip.readUInt16LE(p + 10);
    const compSize = zip.readUInt32LE(p + 20);
    const nameLen = zip.readUInt16LE(p + 28);
    const extraLen = zip.readUInt16LE(p + 30);
    const commentLen = zip.readUInt16LE(p + 32);
    const local = zip.readUInt32LE(p + 42);
    const name = zip.toString("utf8", p + 46, p + 46 + nameLen);
    if (name === member) {
      const lNameLen = zip.readUInt16LE(local + 26);
      const lExtraLen = zip.readUInt16LE(local + 28);
      const start = local + 30 + lNameLen + lExtraLen;
      const data = zip.subarray(start, start + compSize);
      if (method === 0) return Buffer.from(data);
      if (method === 8) return inflateRawSync(data);
      throw new Error(`unsupported zip method ${method}`);
    }
    p += 46 + nameLen + extraLen + commentLen;
  }
  throw new Error(`${member} not found in zip`);
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
  for (const tool of ["yt-dlp", "deno"]) {
    const dest = join(outDir, `${tool}-${target}${exe}`);
    try {
      copyFileSync(findOnPath(tool), dest);
      console.log(`copied ${tool} from PATH -> ${dest}`);
    } catch {
      writeFileSync(dest, `#!/bin/sh\necho "${tool} is not available in this development build" >&2\nexit 1\n`);
      console.log(`${tool} not on PATH, wrote a stub -> ${dest}`);
    }
    chmodSync(dest, 0o755);
  }
  for (const f of ["FFMPEG_LICENSE.txt", "FFMPEG_README.txt", "YTDLP_LICENSE.txt", "DENO_LICENSE.txt"]) {
    writeFileSync(join(outDir, f), "Development build: tools copied from the system PATH.\n");
  }
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

{
  const dest = join(outDir, `yt-dlp-${target}${exe}`);
  writeFileSync(dest, await fetchVerified(t.ytdlp));
  chmodSync(dest, 0o755);
  console.log(`yt-dlp: ${t.ytdlp.url} (sha256 ok) -> ${dest}`);
}
{
  const dest = join(outDir, `deno-${target}${exe}`);
  writeFileSync(dest, unzipMember(await fetchVerified(t.deno), t.deno.zipMember));
  chmodSync(dest, 0o755);
  console.log(`deno: ${t.deno.url} (sha256 ok) -> ${dest}`);
}
writeFileSync(join(outDir, "YTDLP_LICENSE.txt"), await fetchVerified(t.ytdlp_license));
writeFileSync(join(outDir, "DENO_LICENSE.txt"), await fetchVerified(t.deno_license));
console.log("yt-dlp and deno licenses verified");
