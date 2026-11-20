import { describe, expect, it } from "vitest";
import { browseHref, pageWindow, parseState, toQuery, withFilter, withoutFilter } from "./browse";

describe("browse address", () => {
  it("reads filters, paging and sort, ignores everything else, and keeps each filter once", () => {
    const s = parseState(new URLSearchParams("singer=yesudas&album=jeevitha&singer=yesudas&utm=x&page=3&page_size=50&sort=title&bogus=1"));
    expect(s).toEqual({
      filters: [{ facet: "singer", slug: "yesudas" }, { facet: "album", slug: "jeevitha" }],
      page: 3,
      pageSize: 50,
      sort: "title",
    });
  });

  it("falls back to defaults for nonsense", () => {
    expect(parseState({ page: "-1", page_size: "7", sort: "x" })).toEqual({ filters: [], page: 1, pageSize: 20, sort: "newest" });
    expect(parseState({ page: ["2", "3"], singer: ["a", "b"] })).toMatchObject({ page: 2, filters: [{ facet: "singer", slug: "a" }, { facet: "singer", slug: "b" }] });
  });

  it("writes one canonical address per state, leaving defaults out and never an id", () => {
    const s = parseState(new URLSearchParams("page=1&page_size=20&singer=yesudas"));
    expect(toQuery(s)).toBe("singer=yesudas");
    expect(browseHref(s)).toBe("/browse?singer=yesudas");
    expect(browseHref({ filters: [] })).toBe("/browse");
    expect(toQuery(parseState(new URLSearchParams(toQuery({ filters: [{ facet: "raga", slug: "മോഹനം" }], page: 2, pageSize: 10, sort: "title" }))))).toBe(
      "raga=%E0%B4%AE%E0%B5%8B%E0%B4%B9%E0%B4%A8%E0%B4%82&sort=title&page_size=10&page=2",
    );
  });

  it("adds and removes filters in any order, returning to page one", () => {
    const base = parseState({ page: "4" });
    const a = withFilter(base, { facet: "singer", slug: "yesudas" });
    const b = withFilter(a, { facet: "album", slug: "x" });
    expect([a.page, b.filters.map((f) => f.facet)]).toEqual([1, ["singer", "album"]]);
    expect(withFilter(b, { facet: "singer", slug: "yesudas" })).toBe(b);
    expect(withoutFilter(b, 1).filters).toEqual([{ facet: "singer", slug: "yesudas" }]);
    expect(withoutFilter(withoutFilter(b, 1), 0).filters).toEqual([]);
  });

  it("windows page numbers with gaps", () => {
    expect(pageWindow(1, 3)).toEqual([1, 2, 3]);
    expect(pageWindow(10, 20)).toEqual([1, null, 9, 10, 11, null, 20]);
    expect(pageWindow(1, 0)).toEqual([]);
  });
});
