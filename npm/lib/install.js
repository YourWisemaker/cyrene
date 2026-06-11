"use strict";

// postinstall: download the prebuilt cyrene binary that matches this package's
// version and the host platform, verify its checksum, and unpack it into bin/.
// Runs for `npm install`, `pnpm add`, and `yarn add` alike. Network failures
// print actionable guidance instead of leaving a broken install silently.

const fs = require("fs");
const path = require("path");
const os = require("os");
const https = require("https");
const { execFileSync } = require("child_process");
const crypto = require("crypto");

const platform = require("./platform");

const pkg = require("../package.json");
const VERSION = process.env.CYRENE_VERSION || pkg.version;

function log(msg) {
  process.stdout.write(`[cyrene] ${msg}\n`);
}

// Follow redirects (GitHub release assets 302 to a CDN) and resolve to a Buffer.
function fetch(url) {
  return new Promise((resolve, reject) => {
    const req = https.get(
      url,
      { headers: { "User-Agent": "cyrene-npm-installer" } },
      (res) => {
        if (
          res.statusCode >= 300 &&
          res.statusCode < 400 &&
          res.headers.location
        ) {
          res.resume();
          return resolve(fetch(res.headers.location));
        }
        if (res.statusCode !== 200) {
          res.resume();
          return reject(
            new Error(`Download failed: HTTP ${res.statusCode} for ${url}`)
          );
        }
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve(Buffer.concat(chunks)));
      }
    );
    req.on("error", reject);
    req.setTimeout(60000, () => req.destroy(new Error("Download timed out")));
  });
}

function sha256(buf) {
  return crypto.createHash("sha256").update(buf).digest("hex");
}

// Extract a single-file .tar.gz or .zip without third-party deps by shelling
// out to the platform's standard archiver (tar is present on modern Windows).
function extract(archivePath, ext, destDir) {
  if (ext === "zip") {
    if (process.platform === "win32") {
      execFileSync(
        "powershell",
        [
          "-NoProfile",
          "-Command",
          `Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}' -Force`,
        ],
        { stdio: "inherit" }
      );
    } else {
      execFileSync("unzip", ["-o", archivePath, "-d", destDir], {
        stdio: "inherit",
      });
    }
  } else {
    execFileSync("tar", ["-xzf", archivePath, "-C", destDir], {
      stdio: "inherit",
    });
  }
}

function findBinary(rootDir) {
  const name = platform.binaryName();
  const stack = [rootDir];
  while (stack.length) {
    const dir = stack.pop();
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) stack.push(full);
      else if (entry.name === name) return full;
    }
  }
  return null;
}

async function main() {
  const binPath = platform.binaryPath();
  // Skip if already present (e.g. reinstall) unless forced.
  if (fs.existsSync(binPath) && !process.env.CYRENE_FORCE_INSTALL) {
    log("binary already present, skipping download.");
    return;
  }

  const { url, target, ext } = platform.downloadUrl(VERSION);
  log(`installing cyrene v${VERSION} for ${target}...`);

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "cyrene-"));
  const archivePath = path.join(tmp, `cyrene.${ext}`);

  try {
    const archive = await fetch(url);
    fs.writeFileSync(archivePath, archive);

    // Best-effort checksum verification against the published sidecar.
    try {
      const shaText = (await fetch(`${url}.sha256`)).toString("utf8");
      const expected = shaText.trim().split(/\s+/)[0].toLowerCase();
      const actual = sha256(archive);
      if (expected && expected !== actual) {
        throw new Error(
          `checksum mismatch (expected ${expected}, got ${actual})`
        );
      }
      log("checksum verified.");
    } catch (e) {
      log(`checksum verification skipped: ${e.message}`);
    }

    extract(archivePath, ext, tmp);
    const found = findBinary(tmp);
    if (!found) throw new Error("binary not found in downloaded archive");

    fs.mkdirSync(path.dirname(binPath), { recursive: true });
    fs.copyFileSync(found, binPath);
    if (process.platform !== "win32") fs.chmodSync(binPath, 0o755);

    log(`installed to ${binPath}`);
  } catch (err) {
    process.stderr.write(
      `\n[cyrene] Failed to install the prebuilt binary:\n  ${err.message}\n\n` +
        `You can install manually:\n` +
        `  - Linux/macOS/Pi: curl -fsSL https://raw.githubusercontent.com/${platform.REPO}/master/install.sh | bash\n` +
        `  - Windows:        irm https://raw.githubusercontent.com/${platform.REPO}/master/install.ps1 | iex\n` +
        `  - From source:    https://github.com/${platform.REPO}#build-from-source\n\n`
    );
    process.exit(1);
  } finally {
    try {
      fs.rmSync(tmp, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
  }
}

main();
