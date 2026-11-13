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

test("one app asks which app at once, and records it once picked", async ({ page }) => {
  await mockTauri(
    page,
    fixture({
      apps: [
        { exe: "Discord.exe", path: "C:/Apps/Discord.exe", name: "Discord", title: "#general" },
        { exe: "Spotify.exe", path: "C:/Apps/Spotify.exe", name: "Spotify", title: "Spotify" },
      ],
    }),
  );
  await page.goto("/");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("group", { name: "Record" }).getByRole("button", { name: "One app" }).click();

  const list = page.getByRole("listbox", { name: "App to record" });
  await expect(list).toBeVisible();
  // Nothing changes until an app is picked.
  expect(await calls(page, "set_settings")).toHaveLength(0);
  // A window title that only repeats the name is left out.
  await expect(list.getByRole("option", { name: "Spotify" })).toHaveText("Spotify");
  await list.getByRole("option", { name: /Discord/ }).click();
  await expect.poll(async () => (await calls(page, "set_settings")).at(-1)?.args.settings).toMatchObject({ source: "app", appExe: "Discord.exe" });
  await expect(page.getByRole("button", { name: "App to record: Discord" })).toBeVisible();
});

test("turning the sound on plays it once", async ({ page }) => {
  await mockTauri(page, fixture({ settings: { ...fixture().settings, saveSound: false } }));
  await page.goto("/");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("switch", { name: "Sound" }).click();
  await expect.poll(async () => (await calls(page, "set_settings")).at(-1)?.args.settings).toMatchObject({ saveSound: true });
  await expect.poll(async () => (await calls(page, "preview_sound")).length).toBe(1);
});
