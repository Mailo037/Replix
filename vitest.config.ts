import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

// Unit tests for the page's logic (src/**/*.test.ts). Runes in .svelte.ts files run on
// Svelte's browser runtime, as in the app.
export default defineConfig({
  plugins: [svelte()],
  resolve: { conditions: ["browser"] },
  test: { include: ["src/**/*.test.ts"], environment: "node" },
});
