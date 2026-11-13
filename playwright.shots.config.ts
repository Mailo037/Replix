import { defineConfig } from "@playwright/test";
import base from "./playwright.config";

// The product shots in docs/images (tests/shots): the window at 1280×800 on a 2.25× screen,
// so the pictures come out 2880×1800 like a high-density display.
export default defineConfig({
  ...base,
  testDir: "tests/shots",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: "list",
  use: { ...base.use, deviceScaleFactor: 2.25 },
});
