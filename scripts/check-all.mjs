// Runs clippy for Windows, Linux and macOS from one machine, so a change can be checked for
// all three before CI does it.
//
//   node scripts/check-all.mjs            every system
//   node scripts/check-all.mjs linux      only some: windows, linux, macos
//   node scripts/check-all.mjs -j 2       fewer jobs on a machine with little memory
//
// For the other systems it only type-checks (nothing is linked or run). C dependencies
// (ring, objc2) get a stand-in compiler and Linux libraries a stand-in pkg-config: their
// build scripts only need to succeed. The real builds and the tests run in CI on each system.
// Needs the targets once: rustup target add x86_64-unknown-linux-gnu aarch64-apple-darwin
// x86_64-pc-windows-msvc (whichever are not this machine).

import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const args = process.argv.slice(2);
const jobsAt = args.indexOf("-j");
const jobs = jobsAt >= 0 ? ["-j", args[jobsAt + 1]] : [];
const wanted = args.filter((a, i) => !a.startsWith("-") && i !== jobsAt + 1);

const host = process.platform === "win32" ? "windows" : process.platform === "darwin" ? "macos" : "linux";
const TARGETS = {
  windows: "x86_64-pc-windows-msvc",
  linux: "x86_64-unknown-linux-gnu",
  macos: "aarch64-apple-darwin",
};

const tools = mkdtempSync(join(tmpdir(), "replix-check-"));
writeFileSync(
  join(tools, "cc.js"),
  `const fs = require("fs");
const a = process.argv.slice(2);
const o = a.indexOf("-o");
if (o >= 0 && a[o + 1]) fs.writeFileSync(a[o + 1], "");
if (a.includes("-E")) process.stdout.write("clang\\n__clang__\\n");
if (a.includes("--version") || a.includes("-v")) process.stdout.write("clang version 18.0.0\\n");
`,
);
writeFileSync(join(tools, "pkg-config.js"), `if (process.argv.includes("--modversion")) process.stdout.write("99.0.0\\n");\n`);
let pkgConfig;
if (host === "windows") {
  pkgConfig = join(tools, "pkg-config.cmd");
  writeFileSync(pkgConfig, `@node "${join(tools, "pkg-config.js")}" %*\r\n`);
} else {
  pkgConfig = join(tools, "pkg-config");
  writeFileSync(pkgConfig, `#!/bin/sh\nexec node "${join(tools, "pkg-config.js")}" "$@"\n`);
  chmodSync(pkgConfig, 0o755);
}

let failed = [];
for (const system of wanted.length ? wanted : Object.keys(TARGETS)) {
  const target = TARGETS[system];
  if (!target) {
    console.error(`unknown system: ${system}`);
    process.exit(2);
  }
  const env = { ...process.env };
  const cargoArgs = ["clippy", ...jobs, "--all-targets"];
  if (system !== host) {
    const t = target.replace(/-/g, "_");
    env[`CC_${t}`] = `node ${join(tools, "cc.js")}`;
    env[`AR_${t}`] = "ar";
    env.PKG_CONFIG = pkgConfig;
    env.PKG_CONFIG_ALLOW_CROSS = "1";
    cargoArgs.push("--target", target);
  }
  cargoArgs.push("--", "-D", "warnings");
  console.log(`\n== ${system} (${system === host ? "this machine" : target})`);
  const run = spawnSync("cargo", cargoArgs, { cwd: join(root, "src-tauri"), env, stdio: "inherit", shell: host === "windows" });
  if (run.status !== 0) failed.push(system);
}
if (failed.length) {
  console.error(`\nclippy failed for: ${failed.join(", ")}`);
  process.exit(1);
}
console.log("\nclippy is clean on every system");
