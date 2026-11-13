import { describe, expect, it } from "vitest";
import { duration, onColor, position, size } from "./format";
import { startingEdit, withDefault } from "./trackDefaults";
import type { Settings, TrackDefault } from "./api";

describe("format", () => {
  it("writes lengths as m:ss or h:mm:ss", () => {
    expect(duration(5)).toBe("0:05");
    expect(duration(65)).toBe("1:05");
    expect(duration(3725)).toBe("1:02:05");
  });

  it("writes editor positions to a tenth", () => {
    expect(position(12.44)).toBe("0:12.4");
    expect(position(-1)).toBe("0:00.0");
  });

  it("writes sizes in KB, MB or GB", () => {
    expect(size(10)).toBe("1 KB");
    expect(size(5 * 1024 ** 2)).toBe("5 MB");
    expect(size(2.5 * 1024 ** 3)).toBe("2.5 GB");
  });

  it("picks dark text on a light accent and white on a dark one", () => {
    expect(onColor("#C5F26B")).toBe("#17111f");
    expect(onColor("#1d1030")).toBe("#ffffff");
  });
});

describe("track defaults", () => {
  const settings = (trackDefaults: TrackDefault[]) => ({ trackDefaults }) as unknown as Settings;

  it("prefers a program's own default over its kind's", () => {
    const s = settings([
      { key: "app", name: "Apps", enabled: false, volume: 1 },
      { key: "discord.exe", name: "Discord", enabled: true, volume: 0.3 },
    ]);
    expect(startingEdit(s, { kind: "app", name: "Discord", exe: "C:\\Apps\\Discord.exe" })).toEqual({ enabled: true, volume: 0.3 });
    expect(startingEdit(s, { kind: "app", name: "Spotify", exe: "/usr/bin/spotify" })).toEqual({ enabled: false, volume: 1 });
  });

  it("replaces a default with the same key", () => {
    const a = { key: "mic", name: "Mic", enabled: true, volume: 1 };
    expect(withDefault([a], { ...a, volume: 0.5 })).toEqual([{ ...a, volume: 0.5 }]);
  });
});
