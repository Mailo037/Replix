import { expect, test } from "@playwright/test";
import { calls, fixture, mockTauri } from "./tauri";

const since = Date.now() - 125_000;
const recording = fixture({
  status: { state: "recording", target: "Neon Rally", sessionSince: since, encoder: "NVIDIA NVENC", error: null, clipSeconds: 30 },
  liveMarks: { since, markers: [{ at: 42, name: "Boss down", icon: "trophy" }] },
});

test("a running session opens from its tile, and moments are marked while it records", async ({ page }) => {
  await mockTauri(page, recording);
  await page.goto("/");
  await page.getByRole("button", { name: "Open Neon Rally session to mark moments" }).click();
  await expect(page.getByRole("heading", { name: "Neon Rally session" })).toBeVisible();
  await expect(page.getByRole("timer")).toContainText("2:0");

  const list = page.getByRole("list", { name: "Marked moments" });
  await expect(list.getByRole("textbox", { name: "Marker name" })).toHaveValue("Boss down");

  // Noticed a little late: half a minute back.
  await page.getByRole("button", { name: "30 s ago" }).click();
  await expect.poll(async () => (await calls(page, "mark_session")).at(-1)?.args).toMatchObject({ ago: 30 });
  await expect(list.getByRole("listitem")).toHaveCount(2);
  // The new one is chosen to be named: its icons are open.
  const fresh = list.getByRole("listitem").first();
  await expect(fresh.getByRole("radio", { name: "Skull" })).toBeVisible();
  await fresh.getByRole("textbox", { name: "Marker name" }).fill("Clutch");
  await expect.poll(async () => (await calls(page, "edit_session_marker")).at(-1)?.args).toMatchObject({ name: "Clutch" });
  await fresh.getByRole("radio", { name: "Skull" }).click();
  await expect.poll(async () => (await calls(page, "edit_session_marker")).at(-1)?.args).toMatchObject({ name: "Clutch", icon: "skull" });

  // M marks now; the old one goes.
  await page.locator("body").click({ position: { x: 5, y: 300 } });
  await page.keyboard.press("m");
  await expect(list.getByRole("listitem")).toHaveCount(3);
  await list.getByRole("listitem").last().getByRole("button", { name: "Remove marker" }).click();
  await expect(list.getByRole("listitem")).toHaveCount(2);
  await expect(list.getByRole("textbox", { name: "Marker name" }).last()).toHaveValue("Clutch");
});

test("a stopped session opens in the editor once it is saved", async ({ page }) => {
  await mockTauri(page, recording);
  await page.goto("/");
  await page.getByRole("button", { name: "Open Neon Rally session to mark moments" }).click();
  await page.getByRole("button", { name: "Stop" }).click();
  await expect.poll(async () => (await calls(page, "toggle_session")).length).toBe(1);

  // Like Replix: the session ends, is saved, and the library lists it.
  await page.evaluate((created) => {
    window.__emit("status", { state: "recording", target: "Neon Rally", sessionSince: null, encoder: "NVIDIA NVENC", error: null, clipSeconds: 30 });
    window.__emit("session-marks", null);
    window.__emit("save-started", { id: "save-1", kind: "session", title: "Neon Rally session", game: "Neon Rally", created, duration: 125 });
  }, Date.now());
  await expect(page.getByRole("heading", { name: "Saving session…" })).toBeVisible();
  await page.evaluate(() => window.__emit("save-finished", { id: "save-1", item: "a" }));
  await expect(page.getByRole("heading", { name: "Session saved" })).toBeVisible();
  await expect(page.getByText("Your markers are on its timeline.")).toBeVisible();
  await page.getByRole("button", { name: "Open in the editor" }).click();
  await expect(page.getByRole("textbox", { name: "Title" })).toHaveValue("Last-lap overtake");
});
