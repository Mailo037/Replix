// Runs clippy for Windows, Linux and macOS from one machine, so a change can be checked for
// all three before CI does it.
//
//   node scripts/check-all.mjs            every system
//   node scripts/check-all.mjs linux      only some: windows, linux, macos
//   node scripts/check-all.mjs -j 2       fewer jobs on a machine with little memory
//
// For the other systems it only type-checks (nothing is linked or run). C dependencies
// (ring, objc2) get a stand-in compiler and archiver, and Linux libraries a stand-in pkg-config: their
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
const wanted = args.filter((a, i) => !a.startsWith("-") && (jobsAt < 0 || i !== jobsAt + 1));

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
// Preprocessing: cc's compiler check learns it is clang; anything else (Tauri's Windows
// resource script) comes out as it went in.
const input = a.filter((x) => !x.startsWith("-")).pop();
if (a.includes("-E")) {
  if (!input || input.includes("detect_compiler_family")) process.stdout.write("clang\\n__clang__\\n");
  else process.stdout.write(fs.readFileSync(input));
}
if (a.includes("--version") || a.includes("-v")) process.stdout.write("clang version 18.0.0\\n");
`,
);
writeFileSync(join(tools, "pkg-config.js"), `if (process.argv.includes("--modversion")) process.stdout.write("99.0.0\\n");\n`);
// The archiver only has to leave an archive where it was asked to: GNU style (`crs out.a …`)
// or the MSVC style the Windows target uses (`-out:out.lib`).
writeFileSync(
  join(tools, "ar.js"),
  `const fs = require("fs");
const a = process.argv.slice(2);
const out = a.map((x) => /^[-/]out:(.+)$/i.exec(x)?.[1]).find(Boolean) ?? (/^-?[a-zA-Z]+$/.test(a[0] ?? "") ? a[1] : undefined);
if (out) fs.writeFileSync(out, "");
`,
);

/** A one-word command that runs `script` with node: build scripts split a CC with spaces in it their own way. */
function standIn(name, script) {
  if (host === "windows") {
    const path = join(tools, `${name}.cmd`);
    writeFileSync(path, `@node "${join(tools, script)}" %*\r\n`);
    return path;
  }
  const path = join(tools, name);
  writeFileSync(path, `#!/bin/sh\nexec node "${join(tools, script)}" "$@"\n`);
  chmodSync(path, 0o755);
  return path;
}
const pkgConfig = standIn("pkg-config", "pkg-config.js");
const cc = standIn("cc", "cc.js");
const ar = standIn("ar", "ar.js");

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
    env[`CC_${t}`] = cc;
    env[`AR_${t}`] = ar;
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
