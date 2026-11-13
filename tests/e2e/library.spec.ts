import { expect, test } from "@playwright/test";
import { calls, mockTauri } from "./tauri";

test("the library shows the clips by day and game", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Open Last-lap overtake" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open Triple on the ridge" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open Drift into the tunnel" })).toBeVisible();

  await page.getByRole("button", { name: /Hollow Peak/ }).first().click();
  await expect(page.getByRole("button", { name: "Open Triple on the ridge" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open Last-lap overtake" })).toHaveCount(0);
});

test("search finds clips by title", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("searchbox").fill("tunnel");
  await expect(page.getByRole("button", { name: "Open Drift into the tunnel" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open Last-lap overtake" })).toHaveCount(0);
});

test("tags show in the sidebar and filter the library", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("navigation").getByRole("button", { name: /funny/ }).click();
  await expect(page.getByRole("heading", { name: "#funny" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open Drift into the tunnel" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open Last-lap overtake" })).toHaveCount(0);
});

test("search with # finds tags", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("searchbox").fill("#win");
  await expect(page.getByRole("button", { name: "Open Last-lap overtake" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open Drift into the tunnel" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open Triple on the ridge" })).toHaveCount(0);
});

test("a clip gets a tag from its menu and loses it again", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  const tile = page.getByRole("group", { name: "Triple on the ridge" });
  await tile.hover();
  await tile.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Tags" }).click();
  const panel = page.getByRole("dialog", { name: "Tags" });
  const card = tile.getByRole("button", { name: "Open Triple on the ridge" });
  await panel.getByRole("textbox", { name: "Add a tag" }).fill("#Clutch ");
  await page.keyboard.press("Enter");
  await expect(card.getByText("#Clutch")).toBeVisible();
  await expect(page.getByRole("navigation").getByRole("button", { name: /Clutch/ })).toBeVisible();
  // Tags used elsewhere are suggested.
  await panel.getByRole("button", { name: "#funny" }).click();
  await expect(card.getByText("#funny")).toBeVisible();
  await panel.getByRole("button", { name: "Remove the tag Clutch" }).click();
  await expect(card.getByText("#Clutch")).toHaveCount(0);
});

test("several clips are tagged at once", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Select", exact: true }).click();
  await page.getByRole("button", { name: "Select Last-lap overtake" }).click();
  await page.getByRole("button", { name: "Select Triple on the ridge" }).click();
  await page.getByRole("toolbar", { name: "Selected clips" }).getByRole("button", { name: "Tag" }).click();
  await page.getByRole("textbox", { name: "Add a tag" }).fill("highlight");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("navigation").getByRole("button", { name: /highlight\s*2/ })).toBeVisible();
});

test("picked clips are combined into a montage, oldest first", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Select", exact: true }).click();
  const toolbar = page.getByRole("toolbar", { name: "Selected clips" });
  await page.getByRole("button", { name: "Select Last-lap overtake" }).click();
  // One clip is no montage.
  await expect(toolbar.getByRole("button", { name: "Combine" })).toHaveCount(0);
  await page.getByRole("button", { name: "Select Drift into the tunnel" }).click();
  await toolbar.getByRole("button", { name: "Combine" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Combining 2 clips" })).toBeVisible();
  await expect(page.getByText("Montage saved")).toBeVisible();
  const [call] = await calls(page, "combine_clips");
  expect(call.args).toMatchObject({ ids: ["a", "c"], title: "Montage" });
});
