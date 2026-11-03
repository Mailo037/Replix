// Sets the app version everywhere it is written down.
//   npm run set-version 0.2.0
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("Usage: npm run set-version <major.minor.patch>");
  process.exit(1);
}

const edit = (path, change) => writeFileSync(path, change(readFileSync(path, "utf8")));

edit("package.json", (t) => t.replace(/"version": "[^"]+"/, `"version": "${version}"`));
edit("src-tauri/tauri.conf.json", (t) => t.replace(/"version": "[^"]+"/, `"version": "${version}"`));
edit("src-tauri/Cargo.toml", (t) => t.replace(/^version = "[^"]+"/m, `version = "${version}"`));
edit("src-tauri/Cargo.lock", (t) => t.replace(/(name = "replix"\r?\nversion = )"[^"]+"/, `$1"${version}"`));

console.log(`Version ${version}. Next: commit, then git tag v${version} && git push --follow-tags`);
