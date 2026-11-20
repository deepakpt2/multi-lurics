import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_READING, READING_KEY, emptyPane, idsParam, initialPanes, loadReading, ofType, parseIds, saveReading, withType, type Lyric } from "./lyric-panes";

const lyric = (id: string, type: string, language: string): Lyric =>
  ({ id, recording_id: "r1", type, language, script: "Mlym", dir: "ltr", is_primary: id === "a", body: id, revised_at: "2026-01-01T00:00:00Z" }) as Lyric;
const lyrics = [lyric("a", "original", "ml"), lyric("b", "translation", "en"), lyric("c", "translation", "hi"), lyric("d", "transliteration", "ml")];

describe("lyric columns", () => {
  it("start with the main lyric, or with the ones the address names", () => {
    expect(initialPanes(lyrics).map((p) => p.lyricId)).toEqual(["a"]);
    expect(initialPanes(lyrics, ["b", "a"]).map((p) => p.lyricId)).toEqual(["b", "a"]);
    expect(initialPanes(lyrics, ["nope"]).map((p) => p.lyricId)).toEqual(["a"]);
    expect(initialPanes([]).map((p) => p.lyricId)).toEqual([null]);
  });

  it("a new column starts on the first type that still has something to show", () => {
    const [one] = initialPanes(lyrics);
    expect(emptyPane(lyrics, [one])).toMatchObject({ type: "translation", lyricId: null });
    const all = initialPanes(lyrics, ["a", "b", "c"]);
    expect(emptyPane(lyrics, all).type).toBe("transliteration");
    expect(emptyPane([lyric("a", "original", "ml")], [one]).type).toBe("translation");
  });

  it("changing a column's type drops its lyric unless the lyric is of that type", () => {
    const [p] = initialPanes(lyrics, ["b"]);
    expect(withType(lyrics, p, "original")).toMatchObject({ type: "original", lyricId: null });
    expect(withType(lyrics, p, "translation").lyricId).toBe("b");
  });

  it("lists the lyrics of a type and shares the shown ones as an address", () => {
    expect(ofType(lyrics, "translation").map((l) => l.language)).toEqual(["en", "hi"]);
    expect(idsParam([...initialPanes(lyrics, ["a", "c"]), { key: "x", type: "translation", lyricId: null }])).toBe("a,c");
    expect(parseIds("a, b,,c")).toEqual(["a", "b", "c"]);
    expect(parseIds(["a", "b"])).toEqual(["a", "b"]);
    expect(parseIds(undefined)).toEqual([]);
  });
});

describe("reading preferences", () => {
  beforeEach(() => window.localStorage.clear());
  it("round-trip, and bad stored values fall back to the defaults", () => {
    expect(loadReading()).toEqual(DEFAULT_READING);
    saveReading({ size: 4, theme: "sepia" });
    expect(loadReading()).toEqual({ size: 4, theme: "sepia" });
    window.localStorage.setItem(READING_KEY, JSON.stringify({ size: 99, theme: "neon" }));
    expect(loadReading()).toEqual(DEFAULT_READING);
    window.localStorage.setItem(READING_KEY, "not json");
    expect(loadReading()).toEqual(DEFAULT_READING);
  });
});
