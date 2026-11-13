import { expect, test, type Page } from "@playwright/test";
import { calls, fixture, mockTauri } from "./tauri";

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

test("slows the part under the playhead and exports it slow", async ({ page }) => {
  await openEditor(page);
  await playheadTo(page, 2);
  await page.keyboard.press("s");
  const speed = page.getByRole("button", { name: /Speed of this part/ });
  await speed.click();
  await expect(speed).toHaveAccessibleName("Speed of this part: ½×");
  // The part from 2 to 6 s now takes 8.
  await expect(page.locator(".length")).toContainText("0:10.0 long");
  await speed.click();
  await expect(speed).toHaveAccessibleName("Speed of this part: ¼×");
  await expect(page.locator(".length")).toContainText("0:18.0 long");
  await speed.click();
  await expect(page.locator(".length")).toContainText("0:06.0 long");
  await speed.click();
  await page.getByRole("button", { name: "Export", exact: true }).click();
  const [request] = await calls(page, "export_clip");
  expect((request.args.request as { speeds: unknown }).speeds).toEqual([{ start: 2, end: 6, factor: 0.5 }]);
});

test("adds text, edits it and exports its drawing", async ({ page }) => {
  await openEditor(page);
  await page.getByRole("button", { name: "Text", exact: true }).click();
  const panel = page.getByRole("dialog", { name: "Text" });
  const words = panel.getByRole("textbox", { name: "Words" });
  await expect(words).toHaveValue("Your text");
  await words.fill("GG EZ");
  await expect(page.getByRole("img", { name: "GG EZ" })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(panel).toHaveCount(0);
  await expect(page.getByRole("button", { name: /Text “GG EZ” from 0:00.0 to 0:03.0/ })).toBeVisible();
  await page.getByRole("button", { name: "Export", exact: true }).click();
  const [request] = await calls(page, "export_clip");
  const [text] = (request.args.request as { texts: { text: string; png: string; start: number; end: number; x: number; y: number }[] }).texts;
  expect(text).toMatchObject({ text: "GG EZ", start: 0, end: 3, x: 0.5, y: 0.8 });
  expect(text.png).toMatch(/^data:image\/png;base64,/);
});

test("a text can be taken away again", async ({ page }) => {
  await openEditor(page);
  await page.getByRole("button", { name: "Text", exact: true }).click();
  await page.getByRole("dialog", { name: "Text" }).getByRole("button", { name: "Remove text" }).click();
  await expect(page.getByRole("img", { name: "Your text" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: /Text “Your text”/ })).toHaveCount(0);
});

test("says how far opening a long session is", async ({ page }) => {
  await mockTauri(page, fixture({ slowOpen: 3000 }));
  await page.goto("/");
  await page.getByRole("button", { name: "Open Last-lap overtake" }).click();
  const status = page.getByRole("status").filter({ hasText: "Getting the clip ready" });
  await expect(status).toBeVisible();
  await expect(status).toContainText("Copying the audio tracks · 0:03 of 0:06");
  await expect(status.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "50");
  await expect(status).toContainText("so far");
  // Ready: the editor takes its place.
  await expect(page.getByRole("textbox", { name: "Title" })).toHaveValue("Last-lap overtake");
  await expect(status).toHaveCount(0);
});

test("full screen has its own controls: scrub, markers and the player's volume", async ({ page }) => {
  await openEditor(page);
  await page.getByRole("button", { name: "Full screen" }).click();
  const controls = page.getByRole("group", { name: "Player controls" });
  await expect(controls).toBeVisible();

  // Scrubbing along the bar moves the playhead.
  const bar = controls.getByRole("slider", { name: "Position" });
  const box = (await bar.boundingBox())!;
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await expect(controls.locator(".clock")).toContainText("0:03.0 / 0:06.0");
  await bar.focus();
  await page.keyboard.press("Shift+ArrowRight");
  await expect(controls.locator(".clock")).toContainText("0:04.0 / 0:06.0");

  // Muting is the player's: the tracks stay as they are, and the export keeps its sound.
  await controls.getByRole("button", { name: "Mute", exact: true }).click();
  await expect(controls.getByRole("button", { name: "Unmute", exact: true })).toBeVisible();
  await expect(controls.getByRole("slider", { name: "Volume" })).toHaveValue("0");
  await expect.poll(() => page.evaluate(() => [...document.querySelectorAll("audio")].map((a) => a.volume))).toEqual([0, 0, 0]);
  await controls.getByRole("button", { name: "Unmute", exact: true }).click();
  await page.keyboard.press("ArrowDown");
  await expect(controls.getByRole("slider", { name: "Volume" })).toHaveValue("90");

  await controls.getByRole("button", { name: "Mark this moment" }).click();
  await controls.getByRole("button", { name: "Leave full screen" }).click();
  await expect(controls).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Marker at 0:04.0" })).toBeVisible();
  await page.getByRole("button", { name: "Export", exact: true }).click();
  const [request] = await calls(page, "export_clip");
  expect(request.args.request).toMatchObject({ masterVolume: 1 });
});
