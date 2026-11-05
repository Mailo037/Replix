// Collects every text Replix shows, for translating, and checks the translations.
//
//   node scripts/i18n.mjs          writes src/locales/keys.json and reports per language
//   node scripts/i18n.mjs --check  only reports, and fails when a translation breaks a {placeholder}
//
// Texts are found by their calls: t("…") in the page, tr("…") and trf("…") in Rust.
// English is the source and the key; src/locales/en.json holds only plural forms.

import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const check = process.argv.includes("--check");

function files(dir, ext) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...files(path, ext));
    else if (ext.some((e) => name.endsWith(e))) out.push(path);
  }
  return out;
}

/** A JavaScript or Rust string literal's text. */
function unquote(literal) {
  const quote = literal[0];
  const body = literal.slice(1, -1);
  if (quote === '"') return JSON.parse(literal);
  // Single quotes and template literals without ${…}: same escapes, other delimiter.
  return JSON.parse('"' + body.replace(/\\(['`])/g, "$1").replace(/"/g, '\\"') + '"');
}

const keys = new Map();
const page = /\bt(?:Parts)?\(\s*("(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\$]|\\.)*`)/g;
const rust = /\btrf?\(\s*"((?:[^"\\]|\\.)*)"/g;
for (const file of files(join(root, "src"), [".svelte", ".ts"])) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(page)) keys.set(unquote(m[1]), relative(root, file));
}
for (const file of files(join(root, "src-tauri", "src"), [".rs"])) {
  const text = readFileSync(file, "utf8").replace(/#\[cfg\(test\)\][\s\S]*$/, "");
  for (const m of text.matchAll(rust)) keys.set(JSON.parse(`"${m[1]}"`), relative(root, file));
}

const sorted = [...keys.keys()].sort((a, b) => a.localeCompare(b));
writeFileSync(join(root, "src", "locales", "keys.json"), JSON.stringify(sorted, null, 2) + "\n");

const placeholders = (s) => new Set([...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]));
let broken = 0;
const locales = join(root, "src", "locales");
for (const name of readdirSync(locales).sort()) {
  if (!name.endsWith(".json") || name === "keys.json") continue;
  const code = name.replace(/\.json$/, "");
  let catalog;
  try {
    catalog = JSON.parse(readFileSync(join(locales, name), "utf8"));
  } catch (e) {
    broken++;
    console.error(`${name}: not valid JSON (${e.message})`);
    continue;
  }
  let missing = 0;
  for (const key of sorted) {
    const entry = catalog[key];
    if (entry === undefined || entry === "") {
      if (code !== "en") missing++;
      continue;
    }
    const forms = typeof entry === "string" ? [entry] : Object.values(entry);
    const wanted = placeholders(key);
    for (const form of forms) {
      const extra = [...placeholders(form)].filter((p) => !wanted.has(p));
      if (extra.length) {
        broken++;
        console.error(`${name}: "${key}" -> "${form}" uses {${extra.join("}, {")}}`);
      }
    }
  }
  const unused = Object.keys(catalog).filter((k) => !keys.has(k));
  console.log(`${code.padEnd(6)} ${missing ? `${missing} missing` : "complete"}${unused.length ? `, ${unused.length} unused` : ""}`);
}
console.log(`${sorted.length} texts`);
if (check && broken) process.exit(1);
