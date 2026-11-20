import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_PREFS, hide, loadPrefs, move, moveTo, normalize, savePrefs, show, storageKey, visible } from "./home-prefs";

describe("home preferences", () => {
  beforeEach(() => window.localStorage.clear());

  it("starts with every section, in the default order", () => {
    expect(loadPrefs("u1")).toEqual(DEFAULT_PREFS);
    expect(DEFAULT_PREFS.order).toEqual(["editors_picks", "popular_songs", "new_songs", "popular_albums", "new_albums", "popular_artists"]);
  });

  it("repairs what was stored: unknown and repeated keys go, newer sections are added", () => {
    const p = normalize({ order: ["new_albums", "gone", "new_albums", "popular_songs"], hidden: ["popular_songs", "nope"] });
    expect(p.order).toEqual(["new_albums", "popular_songs", "editors_picks", "new_songs", "popular_albums", "popular_artists"]);
    expect(p.hidden).toEqual(["popular_songs"]);
    expect(normalize("garbage")).toEqual(DEFAULT_PREFS);
  });

  it("hides and brings back a section without losing its place", () => {
    let p = hide(DEFAULT_PREFS, "new_songs");
    expect(visible(p)).toEqual(["editors_picks", "popular_songs", "popular_albums", "new_albums", "popular_artists"]);
    p = show(p, "new_songs");
    expect(visible(p)).toEqual(DEFAULT_PREFS.order);
  });

  it("moves a section past its visible neighbour, skipping hidden ones", () => {
    let p = hide(DEFAULT_PREFS, "new_songs");
    p = move(p, "popular_albums", -1); // the neighbour above is popular_songs: new_songs is hidden
    expect(visible(p)).toEqual(["editors_picks", "popular_albums", "popular_songs", "new_albums", "popular_artists"]);
    p = move(p, "popular_albums", -1);
    expect(visible(p)[0]).toBe("popular_albums");
    expect(move(p, "popular_albums", -1)).toBe(p); // already first
  });

  it("drags a section to where another is", () => {
    expect(moveTo(DEFAULT_PREFS, "popular_artists", "editors_picks").order[0]).toBe("popular_artists");
    expect(moveTo(DEFAULT_PREFS, "popular_songs", "new_albums").order).toEqual(["editors_picks", "new_songs", "popular_albums", "new_albums", "popular_songs", "popular_artists"]);
  });

  it("keeps each signed-in user's layout apart", () => {
    savePrefs("u1", hide(DEFAULT_PREFS, "new_albums"));
    expect(loadPrefs("u1").hidden).toEqual(["new_albums"]);
    expect(loadPrefs("u2").hidden).toEqual([]);
    expect(loadPrefs(null).hidden).toEqual([]);
    expect(storageKey("u1")).not.toBe(storageKey("u2"));
  });

  it("falls back to the default when storage holds junk", () => {
    window.localStorage.setItem(storageKey("u1"), "{not json");
    expect(loadPrefs("u1")).toEqual(DEFAULT_PREFS);
  });
});
