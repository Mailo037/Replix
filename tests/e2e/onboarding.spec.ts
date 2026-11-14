import { expect, test } from "@playwright/test";
import { calls, fixture, mockTauri, settings } from "./tauri";

const lastSaved = async (page: import("@playwright/test").Page) => (await calls(page, "set_settings")).at(-1)?.args.settings;

test("the first start sets up in two steps and is not shown again", async ({ page }) => {
  await mockTauri(page, fixture({ settings: { ...settings, onboarded: false } }));
  await page.goto("/");

  await expect(page.getByRole("heading", { name: "Never miss the moment" })).toBeVisible();
  await page.getByRole("group", { name: "Clip length" }).getByRole("button", { name: "1 min" }).click();
  await expect.poll(() => lastSaved(page)).toMatchObject({ bufferSeconds: 60 });
  await page.getByRole("switch", { name: "Record my voice" }).click();
  await expect.poll(() => lastSaved(page)).toMatchObject({ recordMic: false });
  await expect(page.getByRole("meter", { name: "Microphone level" })).toHaveCount(0);

  await page.getByRole("button", { name: "Next", exact: true }).click();
  await expect(page.getByRole("heading", { name: "One key, and it's saved" })).toBeVisible();
  // Trying the key says what it does, with the clip length picked before.
  await page.getByRole("button", { name: /Alt\s*\+\s*C/ }).click();
  await expect(page.getByRole("status").filter({ hasText: "That's it" })).toContainText("1 min");

  await page.getByRole("button", { name: "Open my library" }).click();
  await expect.poll(() => lastSaved(page)).toMatchObject({ onboarded: true });
  await expect(page.getByRole("heading", { name: "One key, and it's saved" })).toHaveCount(0);
});

test("one app asks which app at once during setup", async ({ page }) => {
  await mockTauri(
    page,
    fixture({
      settings: { ...settings, onboarded: false },
      apps: [{ exe: "Discord.exe", path: "C:/Apps/Discord.exe", name: "Discord", title: "#general" }],
    }),
  );
  await page.goto("/");
  await page.getByRole("group", { name: "Record" }).getByRole("button", { name: "One app" }).click();
  const list = page.getByRole("listbox", { name: "App to record" });
  await expect(list).toBeVisible();
  await list.getByRole("option", { name: /Discord/ }).click();
  await expect.poll(() => lastSaved(page)).toMatchObject({ source: "app", appExe: "Discord.exe" });
});

test("skipping setup still opens the library for good", async ({ page }) => {
  await mockTauri(page, fixture({ settings: { ...settings, onboarded: false } }));
  await page.goto("/");
  await page.getByRole("button", { name: "Skip setup" }).click();
  await expect.poll(() => lastSaved(page)).toMatchObject({ onboarded: true });
});
