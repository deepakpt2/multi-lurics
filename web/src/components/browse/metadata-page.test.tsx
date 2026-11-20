import { describe, expect, it } from "vitest";
import { screen, within } from "@testing-library/react";
import { json, mockFetch, renderIntl } from "@/test/utils";
import type { BrowseSong, SongsResponse } from "@/lib/browse";
import { MetadataPage, groupByAlbum } from "./metadata-page";

const disp = (text: string) => ({ text, dir: "ltr", resolution: "primary" }) as never;
const song = (title: string, album?: string, singers = ["Chithra"]): BrowseSong =>
  ({
    page: { locale: "en", slug: title.toLowerCase() },
    title: disp(title),
    year: 1989,
    singers: singers.map((s) => ({ id: s, slug: s.toLowerCase(), name: disp(s), image_id: null })),
    albums: album ? [{ id: album, slug: album.toLowerCase(), name: disp(album), image_id: null }] : [],
    original_language: null,
    translations: [],
  }) as BrowseSong;
const state = { filters: [], page: 1, pageSize: 50, sort: "newest" as const };
const filter = (over = {}) => ({ facet: "singer", slug: "chithra", label: disp("Chithra"), resolved: true, id: "a1", image_id: null, kind: "person", detail: "playback singer", ...over });
const data = (items: BrowseSong[], over: Partial<SongsResponse> = {}) =>
  ({ items, page: 1, page_size: 50, total: items.length, total_pages: 1, filters: [filter()], ...over }) as SongsResponse;

describe("grouping by album", () => {
  it("starts a group wherever the first album changes, keeping the order served", () => {
    const g = groupByAlbum([song("A", "X"), song("B", "X"), song("C", "Y"), song("D"), song("E")]);
    expect(g.map((x) => [x.album?.name.text ?? null, x.songs.map((s) => s.title.text)])).toEqual([["X", ["A", "B"]], ["Y", ["C"]], [null, ["D", "E"]]]);
  });
});

describe("a metadata page", () => {
  it("shows the name, picture slot, basic facts and the songs grouped under their albums", () => {
    mockFetch({ "GET /api/v1/me": () => json(401, {}) });
    renderIntl(<MetadataPage facet="singer" slug="chithra" state={state} result={{ ok: true, data: data([song("Kannum", "Kireedam"), song("Pavizham", "Kireedam"), song("Solo")]) }} />);
    expect(screen.getByRole("heading", { level: 1, name: "Chithra" })).toBeInTheDocument();
    expect(screen.getAllByText("Singers")).toHaveLength(2); // the browse-by links and the page's own label
    expect(screen.getByText("Person · playback singer")).toBeInTheDocument();
    expect(screen.getByText("3 songs")).toBeInTheDocument();
    // The picture slot: a placeholder with an edit button (anyone signed in may add a photo).
    expect(screen.getByRole("img", { name: /Chithra/ })).toBeInTheDocument();

    const album = screen.getByRole("link", { name: "Kireedam" }).closest("section")!;
    expect(within(album).getByRole("heading", { level: 2 })).toBeInTheDocument();
    expect(within(album).getByRole("link", { name: "Kireedam" })).toHaveAttribute("href", "/browse/album/kireedam");
    expect(within(album).getAllByRole("listitem").map((li) => li.textContent)).toEqual([expect.stringContaining("Kannum"), expect.stringContaining("Pavizham")]);
    expect(within(album).getByRole("link", { name: "Kannum" })).toHaveAttribute("href", "/song/kannum");
    // Metadata inside the page links to its own page too.
    expect(within(album).getAllByRole("link", { name: "Chithra" })[0]).toHaveAttribute("href", "/browse/singer/chithra");
    expect(screen.getByRole("heading", { name: "Not on an album" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Filter songs" })).toHaveAttribute("href", "/browse?singer=chithra");
  });

  it("an album's own page lists its songs in one list, with the film's facts", () => {
    mockFetch({ "GET /api/v1/me": () => json(401, {}) });
    renderIntl(<MetadataPage facet="album" slug="kireedam" state={state} result={{ ok: true, data: data([song("Kannum", "Kireedam")], { filters: [filter({ facet: "album", slug: "kireedam", label: disp("Kireedam"), kind: "soundtrack", detail: "1989" })] as never }) }} />);
    expect(screen.getByRole("heading", { level: 1, name: "Kireedam" })).toBeInTheDocument();
    expect(screen.getByText("Soundtrack · 1989")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 2 })).not.toBeInTheDocument();
  });

  it("a coded value such as a devotional form is shown by its translated name", () => {
    mockFetch({ "GET /api/v1/me": () => json(401, {}) });
    renderIntl(<MetadataPage facet="form" slug="bhajan" state={state} result={{ ok: true, data: data([song("Hari", "Bhakti")], { filters: [filter({ facet: "form", slug: "bhajan", label: disp("bhajan"), id: null, kind: null, detail: null })] as never }) }} />);
    expect(screen.getByRole("heading", { level: 1, name: "Bhajan" })).toBeInTheDocument();
  });

  it("says so when there are no songs, and shows an error with a way to retry when the list cannot load", () => {
    mockFetch({ "GET /api/v1/me": () => json(401, {}) });
    const { unmount } = renderIntl(<MetadataPage facet="singer" slug="chithra" state={state} result={{ ok: true, data: data([], { total: 0 }) }} />);
    expect(screen.getByText("No songs with a published lyric yet.")).toBeInTheDocument();
    unmount();
    renderIntl(<MetadataPage facet="singer" slug="chithra" state={state} result={{ ok: false, status: 500 }} />);
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });
});
