import { defineConfig } from "@playwright/test";

// The page in a browser, with a stand-in for Replix's Rust side (tests/e2e/tauri.ts). It
// checks what the page does; recording and FFmpeg are tested in Rust.
export default defineConfig({
  testDir: "tests/e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: "http://localhost:1420",
    viewport: { width: 1280, height: 800 },
    colorScheme: "dark",
    trace: "off",
  },
  webServer: {
    command: "npx vite --port 1420 --strictPort",
    url: "http://localhost:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
