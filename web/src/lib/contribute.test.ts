import { describe, expect, it } from "vitest";
import { buildSubmission, contributeHref, parsePrefill, type Draft } from "./contribute";

describe("prefill", () => {
  it("an album page locks the album; a deity page makes it devotional with the deity locked", () => {
    const a = parsePrefill({ album: "al1", album_name: "Kireedam" });
    expect(a.album).toEqual([{ id: "al1", name: "Kireedam", locked: true }]);
    expect([...a.locked]).toEqual(["album"]);
    const d = parsePrefill({ deity: "Hanuman", form: "chalisa" });
    expect(d.category).toBe("devotional");
    expect(d.deity).toEqual([{ name: "Hanuman", locked: true }]);
    expect([...d.locked].sort()).toEqual(["category", "deity", "form"]);
  });
  it("ignores half-given references and nothing locks without a value", () => {
    const p = parsePrefill({ singer: "x" });
    expect(p.singers).toEqual([]);
    expect(p.locked.size).toBe(0);
    expect(p.category).toBeUndefined();
  });
  it("builds the address from what a page knows", () => {
    expect(contributeHref({ album: "a b", album_name: "Ki ree", year: undefined })).toBe("/contribute?album=a+b&album_name=Ki+ree");
    expect(contributeHref({})).toBe("/contribute");
  });
});

const draft = (over: Partial<Draft> = {}): Draft => ({
  title: " Kannum ", year: "1989", album: [{ id: "al1", name: "Kireedam" }], singers: [{ name: "Chithra" }], composers: [], lyricists: [], category: "mainstream",
  religion: "hindu", deity: [{ name: "Hanuman" }], raga: [], form: "bhajan", source: "x", occasions: "a, b", tags: [{ name: "film" }], youtube: " https://youtu.be/dQw4w9WgXcQ ",
  blocks: [{ key: "1", language: "ml", script: "", type: "transliteration", body: "kannum", title: "കണ്ണും" }, { key: "2", language: "en", script: "", type: "translation", body: "  ", title: "" }], ...over,
});

describe("submission", () => {
  it("sends ids for chosen things and names for new ones, drops blank blocks and devotional-only fields", () => {
    const s = buildSubmission(draft());
    expect(s).toMatchObject({ title: "Kannum", year: 1989, album: { id: "al1", name: "Kireedam" }, singers: [{ id: null, name: "Chithra" }], religion: "", deity: "", devotional_form: "", occasions: [], tags: ["film"], youtube: "https://youtu.be/dQw4w9WgXcQ", allow_duplicate: false });
    expect(s.lyrics).toEqual([{ type: "transliteration", language: "ml", script: "", body: "kannum", title: "കണ്ണും" }]);
  });
  it("keeps devotional fields for a devotional song", () => {
    expect(buildSubmission(draft({ category: "devotional" }), true)).toMatchObject({ religion: "hindu", deity: "Hanuman", devotional_form: "bhajan", source_work: "x", occasions: ["a", "b"], allow_duplicate: true });
  });
});
