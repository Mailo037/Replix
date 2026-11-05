import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * Replix in other languages. English is the source: `t("Save clip")` looks the English
 * wording up in `src/locales/<code>.json` (shared with Replix's Rust side) and falls back
 * to English where a translation is missing. `{name}` parts are filled in from `params`.
 * A text with a count has plural forms in the catalogs ("one", "few", "other" and so on);
 * the count is `params.n`.
 *
 * Call `t` where the text is shown (in the markup, or in a `$derived`), not once in a
 * component's script: then it follows a change of language.
 */

type Forms = Partial<Record<Intl.LDMLPluralRule, string>>;
type Catalog = Record<string, string | Forms>;

// English comes with the page (it holds only plural forms); another language is loaded
// when it is picked. keys.json is the list of texts for translators, not a language.
const english = import.meta.glob<Catalog>("../locales/en.json", { eager: true, import: "default" });
const loaders = import.meta.glob<Catalog>(["../locales/*.json", "!../locales/en.json", "!../locales/keys.json"], {
  import: "default",
});
const catalogs: Record<string, Catalog> = { en: Object.values(english)[0] ?? {} };

/** Loads a language's catalog; says whether Replix has that language. */
async function load(code: string): Promise<boolean> {
  if (catalogs[code]) return true;
  const loader = loaders[`../locales/${code}.json`];
  if (!loader) return false;
  try {
    catalogs[code] = await loader();
    return true;
  } catch {
    return false;
  }
}

/** Every language Replix speaks, each named in itself. */
export const languages = [
  { code: "en", name: "English" },
  { code: "de", name: "Deutsch" },
  { code: "es", name: "Español" },
  { code: "fr", name: "Français" },
  { code: "it", name: "Italiano" },
  { code: "pt-BR", name: "Português (Brasil)" },
  { code: "pl", name: "Polski" },
  { code: "ru", name: "Русский" },
  { code: "tr", name: "Türkçe" },
  { code: "ja", name: "日本語" },
  { code: "ko", name: "한국어" },
  { code: "zh-CN", name: "简体中文" },
] as const;

class Language {
  code = $state("en");
}

const current = new Language();
const plurals = new Map<string, Intl.PluralRules>();

function pluralRule(code: string, n: number): Intl.LDMLPluralRule {
  let rules = plurals.get(code);
  if (!rules) {
    rules = new Intl.PluralRules(code);
    plurals.set(code, rules);
  }
  return rules.select(n);
}

function pick(entry: string | Forms | undefined, code: string, n: unknown): string | undefined {
  if (typeof entry === "string") return entry || undefined;
  if (!entry) return undefined;
  const count = typeof n === "number" ? n : 0;
  return entry[pluralRule(code, count)] || entry.other || undefined;
}

export function t(text: string, params?: Record<string, string | number>): string {
  const code = current.code;
  const out = pick(catalogs[code]?.[text], code, params?.n) ?? pick(catalogs.en?.[text], "en", params?.n) ?? text;
  if (!params) return out;
  return out.replace(/\{(\w+)\}/g, (whole, name: string) => (name in params ? String(params[name]) : whole));
}

/**
 * A translated sentence cut where markup goes: `{name}` parts not in `params` come back as
 * pieces of their own, e.g. "{key}", so the markup can put an element there:
 *
 *   {#each tParts("Press {key} to save", {}) as part}{#if part === "{key}"}<kbd>Alt+C</kbd>{:else}{part}{/if}{/each}
 */
export function tParts(text: string, params: Record<string, string | number> = {}): string[] {
  return t(text, params)
    .split(/(\{\w+\})/)
    .filter((part) => part !== "");
}

/** The language in use, for dates and numbers: British English for English, as before. */
export function locale(): string {
  return current.code === "en" ? "en-GB" : current.code;
}

export const i18n = {
  get code() {
    return current.code;
  },
  async set(code: string) {
    const known = await load(code);
    current.code = known ? code : "en";
    document.documentElement.lang = current.code;
  },
  /** Takes the language Replix uses and follows its changes. */
  async init() {
    try {
      await i18n.set(await invoke<string>("ui_language"));
    } catch {
      // Outside Replix, say in a plain browser: English.
    }
    await listen<string>("language", (e) => i18n.set(e.payload)).catch(() => {});
  },
};
