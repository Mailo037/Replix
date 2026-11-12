import { expect, test } from "@playwright/test";
import { calls, fixture, mockTauri } from "./tauri";

/** Opens the settings with the page claiming it can (or cannot) play HEVC. */
async function openSettings(page: import("@playwright/test").Page, hevcPlays: boolean, codecs: ("h264" | "hevc")[]) {
  await page.addInitScript((plays: boolean) => {
    const original = HTMLMediaElement.prototype.canPlayType;
    HTMLMediaElement.prototype.canPlayType = function (type: string) {
      if (type.includes("hvc1")) return plays ? "probably" : "";
      return original.call(this, type);
    };
  }, hevcPlays);
  await mockTauri(page, fixture({ codecs }));
  await page.goto("/");
  await page.getByRole("button", { name: "Settings" }).click();
}

test("offers HEVC where it can be recorded and played", async ({ page }) => {
  await openSettings(page, true, ["h264", "hevc"]);
  const format = page.getByRole("group", { name: "Video format" });
  await expect(format).toBeVisible();
  await format.getByRole("button", { name: "HEVC" }).click();
  await expect.poll(async () => (await calls(page, "set_settings")).at(-1)?.args.settings).toMatchObject({ codec: "hevc" });
  await expect(page.getByText("Exports are still H.264")).toBeVisible();
});

test("does not offer HEVC the editor could not play", async ({ page }) => {
  await openSettings(page, false, ["h264", "hevc"]);
  await expect(page.getByRole("heading", { name: "Recording" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Video format" })).toHaveCount(0);
});

test("does not offer HEVC without an encoder for it", async ({ page }) => {
  await openSettings(page, true, ["h264"]);
  await expect(page.getByRole("heading", { name: "Recording" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Video format" })).toHaveCount(0);
});
