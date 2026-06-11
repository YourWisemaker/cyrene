"use strict";

// Shared platform -> Rust target mapping used by both the postinstall
// downloader and the bin launcher so they always agree on file locations.

const os = require("os");
const path = require("path");

const REPO = "YourWisemaker/cyrene";

// Map Node's process.platform + process.arch to the Rust target triple and the
// release asset format. Raspberry Pi (32-bit) reports arch "arm".
function resolveTarget() {
  const platform = process.platform;
  const arch = process.arch;

  let target;
  let ext;

  if (platform === "linux") {
    ext = "tar.gz";
    // Detect musl (Alpine) so we fetch the static build.
    const isMusl = detectMusl();
    if (arch === "x64") {
      target = isMusl
        ? "x86_64-unknown-linux-musl"
        : "x86_64-unknown-linux-gnu";
    } else if (arch === "arm64") {
      target = isMusl
        ? "aarch64-unknown-linux-musl"
        : "aarch64-unknown-linux-gnu";
    } else if (arch === "arm") {
      // Raspberry Pi 32-bit (armv7 hard-float).
      target = "armv7-unknown-linux-gnueabihf";
    }
  } else if (platform === "darwin") {
    ext = "tar.gz";
    if (arch === "x64") target = "x86_64-apple-darwin";
    else if (arch === "arm64") target = "aarch64-apple-darwin";
  } else if (platform === "win32") {
    ext = "zip";
    if (arch === "x64") target = "x86_64-pc-windows-msvc";
    else if (arch === "arm64") target = "aarch64-pc-windows-msvc";
  }

  if (!target) {
    throw new Error(
      `Unsupported platform/arch: ${platform}/${arch}. ` +
        `Build from source: https://github.com/${REPO}#build-from-source`
    );
  }

  return { target, ext, platform };
}

// Best-effort musl detection. ldd/report output mentions "musl" on Alpine.
function detectMusl() {
  try {
    const { execSync } = require("child_process");
    const out = execSync("ldd --version 2>&1 || true", {
      encoding: "utf8",
    });
    return /musl/i.test(out);
  } catch {
    // Some Node builds expose report.glibcVersionRuntime when glibc is present.
    try {
      const report = process.report && process.report.getReport();
      const fields = report && report.header && report.header.glibcVersionRuntime;
      return !fields;
    } catch {
      return false;
    }
  }
}

function binaryName() {
  return process.platform === "win32" ? "cyrene.exe" : "cyrene";
}

function binaryPath() {
  return path.join(__dirname, "..", "bin", binaryName());
}

function downloadUrl(version) {
  const { target, ext } = resolveTarget();
  const v = String(version).replace(/^v/, "");
  return {
    url: `https://github.com/${REPO}/releases/download/v${v}/cyrene-${target}.${ext}`,
    target,
    ext,
  };
}

module.exports = {
  REPO,
  resolveTarget,
  binaryName,
  binaryPath,
  downloadUrl,
  tmpDir: () => os.tmpdir(),
};
