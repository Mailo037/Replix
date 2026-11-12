import { expect, test } from "@playwright/test";
import { mockTauri } from "./tauri";

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
