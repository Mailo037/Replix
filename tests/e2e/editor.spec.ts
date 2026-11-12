import { expect, test, type Page } from "@playwright/test";
import { calls, mockTauri } from "./tauri";

async function openEditor(page: Page) {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Open Last-lap overtake" }).click();
  await expect(page.getByRole("textbox", { name: "Title" })).toHaveValue("Last-lap overtake");
  // The video is loaded once its length is known; the clock shows it.
  await expect(page.locator(".clock")).toContainText("/ 0:06.0");
}

/** Moves the playhead with the arrow keys: Shift moves a second. */
async function playheadTo(page: Page, seconds: number) {
  await page.locator("body").click({ position: { x: 5, y: 400 } });
  for (let i = 0; i < seconds; i++) await page.keyboard.press("Shift+ArrowRight");
}

test("opens a clip with its tracks", async ({ page }) => {
  await openEditor(page);
  await expect(page.getByRole("group", { name: "Neon Rally" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Discord" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Microphone" })).toBeVisible();
});

test("splits, removes a part and undoes it", async ({ page }) => {
  await openEditor(page);
  await playheadTo(page, 2);
  await page.keyboard.press("s");
  await expect(page.locator(".length")).toContainText("2 of 2 parts");
  // Changes made in quick succession become one step to undo; these are two.
  await page.waitForTimeout(500);
  await page.keyboard.press("Delete");
  await expect(page.locator(".length")).toContainText("1 of 2 parts");
  await expect(page.locator(".length")).toContainText("0:02.0 long");

  await page.keyboard.press("Control+z");
  await expect(page.locator(".length")).toContainText("2 of 2 parts");
  await page.keyboard.press("Control+y");
  await expect(page.locator(".length")).toContainText("1 of 2 parts");
});

test("the last part stays", async ({ page }) => {
  await openEditor(page);
  await expect(page.getByRole("button", { name: "Remove", exact: true })).toBeDisabled();
  await playheadTo(page, 1);
  await page.keyboard.press("Delete");
  await expect(page.locator(".length")).toContainText("0:06.0 long");
});

test("marks a moment and names it", async ({ page }) => {
  await openEditor(page);
  await playheadTo(page, 3);
  await page.keyboard.press("m");
  const marker = page.getByRole("button", { name: "Marker at 0:03.0" });
  await expect(marker).toBeVisible();
  await marker.click({ button: "right" });
  await page.getByRole("textbox", { name: "Marker name" }).fill("Overtake");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "Marker Overtake at 0:03.0" })).toBeVisible();
  await expect.poll(async () => (await calls(page, "set_markers")).at(-1)?.args).toEqual({
    id: "a",
    markers: [{ at: 3, name: "Overtake" }],
  });
});

test("mutes a track and exports what is left", async ({ page }) => {
  await openEditor(page);
  await page.getByRole("button", { name: "Mute Discord" }).click();
  await page.keyboard.press("o");
  await page.getByRole("button", { name: "Export", exact: true }).click();
  await expect(page.getByText("Export finished")).toBeVisible();
  const [request] = await calls(page, "export_clip");
  expect(request.args).toMatchObject({
    id: "a",
    title: "Last-lap overtake",
    request: { preset: "original", trimStart: 0, trimEnd: 0.5, vertical: false },
  });
  expect((request.args.request as { tracks: { enabled: boolean }[] }).tracks.map((t) => t.enabled)).toEqual([true, false, true]);
});

test("switches to 9:16 and moves the frame with the keys", async ({ page }) => {
  await openEditor(page);
  await page.getByRole("button", { name: "9:16", exact: true }).click();
  const frame = page.getByRole("button", { name: /9:16 frame/ });
  await expect(frame).toBeVisible();
  await frame.focus();
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("Control+s");
  await expect.poll(async () => (await calls(page, "save_draft")).length).toBe(1);
  const [saved] = await calls(page, "save_draft");
  const draft = saved.args.draft as { vertical: boolean; cropX: number };
  expect(draft.vertical).toBe(true);
  expect(draft.cropX).toBeCloseTo(0.45);
});

test("the export menu offers every format", async ({ page }) => {
  await openEditor(page);
  await page.getByRole("button", { name: "More export options" }).click();
  for (const name of ["Full quality", "For Discord", "Separate tracks", "GIF"]) {
    await expect(page.getByRole("menuitem", { name: new RegExp(name) })).toBeVisible();
  }
  await page.getByRole("menuitem", { name: /For Discord/ }).click();
  const [request] = await calls(page, "export_clip");
  expect(request.args).toMatchObject({ request: { preset: "discord" } });
});
