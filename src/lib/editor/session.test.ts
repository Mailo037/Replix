import { describe, expect, it, vi } from "vitest";
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
      speeds: [{ start: 5, end: 7, factor: 0.5 }],
      texts: [{ text: "GG", x: 0.5, y: 0.8, size: 0.08, height: 0.12, start: 1, end: 4, png: "data:image/png;base64," }],
    };
    const ed = new EditorSession("x");
    ed.load(data(draft), null);
    expect(ed.draft()).toEqual(draft);
    // 1–3 and 5–9 kept, 5–7 at half speed.
    expect(ed.length).toBe(8);
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

  /** A stand-in for a media element: plays on only when told to, seeks at once. */
  function media(readyState = 4) {
    return {
      currentTime: 0,
      paused: true,
      seeking: false,
      readyState,
      seeks: 0,
      play() {
        this.paused = false;
        return Promise.resolve();
      },
      pause() {
        this.paused = true;
      },
    };
  }

  function playing() {
    const ed = new EditorSession("x");
    ed.load(data(), null);
    const video = media();
    const audios = [media(), media()];
    for (const a of audios) {
      let at = 0;
      Object.defineProperty(a, "currentTime", {
        get: () => at,
        set: (v: number) => {
          at = v;
          a.seeks++;
        },
      });
    }
    ed.video = video as unknown as HTMLVideoElement;
    ed.audios = audios as unknown as HTMLAudioElement[];
    return { ed, video, audios };
  }

  it("holds the tracks while the picture is seeking, instead of pulling them back over and over", async () => {
    const { ed, video, audios } = playing();
    await ed.play();
    expect(audios.every((a) => !a.paused)).toBe(true);

    // A jump over a removed part: the picture seeks for a while, the tracks wait for it.
    ed.seek(6);
    video.seeking = true;
    const seeks = audios[0].seeks;
    for (let frame = 0; frame < 30; frame++) ed.follow();
    expect(audios.every((a) => a.paused)).toBe(true);
    expect(audios[0].seeks).toBe(seeks);

    // The picture plays on: the tracks start again where it is, once.
    video.seeking = false;
    video.currentTime = 6.05;
    ed.follow();
    expect(audios.every((a) => !a.paused && a.currentTime === 6.05)).toBe(true);
    expect(audios[0].seeks).toBe(seeks + 1);
  });

  it("leaves a track that was just moved alone while it lands", async () => {
    let clock = 0;
    const now = vi.spyOn(performance, "now").mockImplementation(() => clock);
    const { ed, video, audios } = playing();
    await ed.play();
    const seeks = audios[0].seeks;
    clock = 1000;
    // Slightly off is fine; far off is corrected, then given time to land.
    video.currentTime = 0.1;
    ed.follow();
    expect(audios[0].seeks).toBe(seeks);
    video.currentTime = 1;
    ed.follow();
    expect(audios[0].seeks).toBe(seeks + 1);
    clock = 1100;
    video.currentTime = 1.5;
    ed.follow();
    expect(audios[0].seeks).toBe(seeks + 1);
    clock = 2000;
    ed.follow();
    expect(audios[0].seeks).toBe(seeks + 2);
    now.mockRestore();
  });
});
