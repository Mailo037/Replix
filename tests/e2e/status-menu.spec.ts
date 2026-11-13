import { expect, test, type Page } from "@playwright/test";
import { calls, fixture, mockTauri, settings } from "./tauri";

// The recording menu in the title bar: what to record is a list of three, and screens and apps
// show their choices in a submenu beside it.

async function openMenu(page: Page) {
  await page.goto("/");
  await page.getByRole("button", { name: /^Recording: / }).click();
}

const lastSettings = async (page: Page) => (await calls(page, "set_settings")).at(-1)?.args.settings;

test("a display is picked from the submenu, each shown with a picture", async ({ page }) => {
  await mockTauri(
    page,
    fixture({
      monitors: [
        { index: 0, width: 2560, height: 1440, primary: true },
        { index: 1, width: 1920, height: 1080, primary: false },
      ],
    }),
  );
  await openMenu(page);
  await page.getByRole("button", { name: /Whole screen/ }).click();

  const menu = page.getByRole("menu", { name: "Whole screen" });
  await expect(menu.getByRole("menuitemradio")).toHaveCount(2);
  await expect(menu.locator("img")).toHaveCount(2);
  expect((await calls(page, "monitor_preview")).map((c) => c.args.index).sort()).toEqual([0, 1]);

  await menu.getByRole("menuitemradio", { name: /Display 2/ }).click();
  await expect.poll(() => lastSettings(page)).toMatchObject({ source: "screen", monitor: 1 });
  await expect(menu).toHaveCount(0);
  await expect(page.getByRole("button", { name: /Whole screen/ })).toContainText("Display 2");
});

test("an app is picked from the submenu", async ({ page }) => {
  await mockTauri(
    page,
    fixture({
      apps: [
        { exe: "Discord.exe", path: "C:/Apps/Discord.exe", name: "Discord", title: "#general" },
        { exe: "Spotify.exe", path: "C:/Apps/Spotify.exe", name: "Spotify", title: "Spotify" },
      ],
    }),
  );
  await openMenu(page);
  await page.getByRole("button", { name: /One app/ }).click();

  const menu = page.getByRole("menu", { name: "One app" });
  await expect(menu.getByRole("menuitemradio")).toHaveCount(2);
  // A window title that only repeats the app's name is not shown twice.
  await expect(menu.getByRole("menuitemradio", { name: "Spotify" })).toHaveText("Spotify");

  await menu.getByRole("menuitemradio", { name: /Discord/ }).click();
  await expect.poll(() => lastSettings(page)).toMatchObject({ source: "app", appExe: "Discord.exe" });
  await expect(page.getByRole("button", { name: /One app/ })).toContainText("Discord");
});

test("the app picked before is marked, whatever the case of its name", async ({ page }) => {
  await mockTauri(
    page,
    fixture({
      settings: { ...settings, source: "app", appExe: "discord.exe" },
      apps: [{ exe: "Discord.exe", path: "C:/Apps/Discord.exe", name: "Discord", title: "#general" }],
    }),
  );
  await openMenu(page);
  await page.getByRole("button", { name: /One app/ }).click();
  await expect(page.getByRole("menuitemradio", { name: /Discord/ })).toHaveAttribute("aria-checked", "true");
});

test("the submenus work from the keyboard", async ({ page }) => {
  await mockTauri(page);
  await openMenu(page);
  const row = page.getByRole("button", { name: /Whole screen/ });
  await row.focus();
  await page.keyboard.press("ArrowLeft");
  const item = page.getByRole("menu", { name: "Whole screen" }).getByRole("menuitemradio").first();
  await expect(item).toBeFocused();

  // Escape closes the submenu first and goes back to its row, then the menu.
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu")).toHaveCount(0);
  await expect(row).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(row).toHaveCount(0);
});

test("games are picked straight from the list", async ({ page }) => {
  await mockTauri(page, fixture({ settings: { ...settings, source: "screen" } }));
  await openMenu(page);
  await page.getByRole("button", { name: /Games/ }).click();
  await expect.poll(() => lastSettings(page)).toMatchObject({ source: "game" });
  await expect(page.getByRole("menu")).toHaveCount(0);
});

test("a long microphone name fits its select, and the list shows it whole", async ({ page }) => {
  const long = "Headset Microphone (Oculus Virtual Audio Device)";
  await mockTauri(
    page,
    fixture({
      settings: { ...settings, micDevice: "oculus" },
      mics: [
        { id: "usb", name: "USB Headset", default: true },
        { id: "oculus", name: long, default: false },
      ],
    }),
  );
  await page.goto("/");
  await page.getByRole("button", { name: "Settings" }).click();
  const select = page.getByRole("button", { name: `Microphone: ${long}` });
  await expect(select).toBeVisible();
  const cut = await select.locator(".current").evaluate((el) => el.scrollWidth > el.clientWidth + 1);
  expect(cut).toBe(false);

  await select.click();
  const option = page.getByRole("option", { name: long });
  await expect(option).toBeVisible();
  // The list stays within the window, under the right edge of its button.
  const [list, button] = [await page.getByRole("listbox", { name: "Microphone" }).boundingBox(), await select.boundingBox()];
  expect(list && button && Math.abs(list.x + list.width - (button.x + button.width))).toBeLessThan(2);
});
