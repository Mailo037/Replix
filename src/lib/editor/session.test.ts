import { describe, expect, it } from "vitest";
import type { EditorData, Settings } from "../api";
import { EditorSession } from "./session.svelte";

const data = (draft: EditorData["draft"] = null): EditorData => ({
  id: "x",
  path: "/clip.mp4",
  title: "Clip",
  game: "Game",
  created: 0,
  duration: 10,
  width: 1920,
  height: 1080,
  tracks: [
    { name: "Game", kind: "game", audio: "/g.m4a" },
    { name: "Microphone", kind: "mic", audio: "/m.m4a" },
  ],
  filmstrip: null,
  draft,
  markers: [{ at: 4 }],
});

describe("EditorSession", () => {
  it("starts tracks as the settings say when there is no draft", () => {
    const ed = new EditorSession("x");
    const settings = { trackDefaults: [{ key: "mic", name: "Microphone", enabled: false, volume: 0.5 }] } as unknown as Settings;
    ed.load(data(), settings);
    expect(ed.trimEnd).toBe(10);
    expect(ed.edits).toEqual([
      { enabled: true, volume: 1 },
      { enabled: false, volume: 0.5 },
    ]);
  });

  it("gives back the draft it was loaded from", () => {
    const draft = {
      trimStart: 1,
      trimEnd: 9,
      tracks: [
        { enabled: true, volume: 1 },
        { enabled: true, volume: 0.2 },
      ],
      cuts: [[3, 5]] as [number, number][],
      splits: [3, 5],
      vertical: true,
      cropX: 0.2,
      cropY: 0.5,
      cropScale: 0.8,
      masterVolume: 1.5,
    };
    const ed = new EditorSession("x");
    ed.load(data(draft), null);
    expect(ed.draft()).toEqual(draft);
    expect(ed.length).toBe(6);
  });

  it("removes and restores the part under the playhead, but not the last one", () => {
    const ed = new EditorSession("x");
    ed.load(data(), null);
    expect(ed.togglePart()).toBe("last");
    ed.current = 5;
    ed.splitHere();
    ed.current = 6;
    expect(ed.togglePart()).toBe("removed");
    expect(ed.length).toBe(5);
    expect(ed.togglePart()).toBe("restored");
    expect(ed.length).toBe(10);
  });

  it("sets and takes away a marker at the playhead", () => {
    const ed = new EditorSession("x");
    ed.load(data(), null);
    ed.current = 4.1;
    expect(ed.onMarker).toBe(true);
    ed.toggleMarker();
    expect(ed.markers).toEqual([]);
    ed.current = 2;
    ed.toggleMarker();
    expect(ed.markers).toEqual([{ at: 2 }]);
  });
});
