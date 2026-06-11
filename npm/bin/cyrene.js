#!/usr/bin/env node
"use strict";

// Thin launcher: forwards all args/stdio to the native cyrene binary that the
// postinstall step downloaded. Keeps `npx cyrene ...` and a global install on
// PATH behaving exactly like the native binary.

const { spawnSync } = require("child_process");
const fs = require("fs");
const platform = require("../lib/platform");

const binPath = platform.binaryPath();

if (!fs.existsSync(binPath)) {
  process.stderr.write(
    "[cyrene] native binary is missing. Re-run the install:\n" +
      "  npm rebuild cyrene   (or)   npm install -g cyrene\n"
  );
  process.exit(1);
}

const result = spawnSync(binPath, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  process.stderr.write(`[cyrene] failed to launch binary: ${result.error.message}\n`);
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
