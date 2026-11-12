import { describe, expect, it } from "vitest";
import { History } from "./history.svelte";

describe("History", () => {
  it("undoes and redoes in order", () => {
    const h = new History();
    h.commit("a");
    h.commit("b");
    h.commit("c");
    expect(h.undo()).toBe("b");
    expect(h.undo()).toBe("a");
    expect(h.undo()).toBeNull();
    expect(h.redo()).toBe("b");
    expect(h.committed).toBe("b");
  });

  it("forgets what was undone once something new happens", () => {
    const h = new History();
    h.reset("a");
    h.commit("b");
    h.undo();
    h.commit("c");
    expect(h.canRedo).toBe(false);
    expect(h.undo()).toBe("a");
  });

  it("keeps a limited number of steps", () => {
    const h = new History(2);
    for (const s of ["a", "b", "c", "d"]) h.commit(s);
    expect(h.undo()).toBe("c");
    expect(h.undo()).toBe("b");
    expect(h.undo()).toBeNull();
  });
});
