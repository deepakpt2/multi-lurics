import { beforeEach, describe, expect, it } from "vitest";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { loadPrefs } from "@/lib/home-prefs";
import { HomeSections } from "./home-sections";

const me = (id: string) => ({ id, email: `${id}@example.com`, display_name: id, roles: [], email_verified: true });
const title = (text: string) => ({ text, dir: "ltr", resolution: "primary" });
const empty = ["editors_picks", "popular_songs", "new_songs", "popular_albums", "new_albums", "popular_artists"].map((key) => ({ key, items: [] }));
const withData = empty.map((s) =>
  s.key === "popular_songs"
    ? {
        ...s,
        items: [{ rank: 1, id: "s1", title: title("Karunamayane"), page: { locale: "en", slug: "karunamayane" }, singers: [{ id: "a1", slug: "yesudas", name: title("Yesudas") }], albums: [{ id: "b1", slug: "hits", name: title("Hits") }], views: 120, rating_average: 4.5, rating_count: 12, comments: 3 }],
      }
    : s,
);

beforeEach(() => window.localStorage.clear());

describe("home sections", () => {
  it("on a new install shows every title with a nothing-to-see message and a way to add", async () => {
    mockFetch({ "GET /api/v1/me": () => problem(401, "unauthenticated"), "GET /api/v1/home": () => json(200, { generated_at: "2026-01-01T00:00:00Z", sections: empty }) });
    renderIntl(<HomeSections />);
    for (const name of ["Editors' picks", "Most popular songs", "New songs", "Popular albums", "New albums", "Most viewed artists"])
      expect(await screen.findByRole("heading", { name })).toBeInTheDocument();
    expect(screen.getAllByText("Nothing to see here yet.")).toHaveLength(6); // the editors' picks section explains itself instead of offering to add
    expect(screen.getAllByRole("link", { name: "Add a song now" })[0]).toHaveAttribute("href", "/compositions/new");
    expect(screen.getAllByRole("link", { name: "Add an album now" })).toHaveLength(2);
  });

  it("shows ranked songs with their numbers", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me("u1")), "GET /api/v1/home": () => json(200, { generated_at: null, sections: withData }) });
    renderIntl(<HomeSections />);
    const section = (await screen.findByRole("heading", { name: "Most popular songs" })).closest("section")!;
    expect(within(section).getByRole("link", { name: "Karunamayane" })).toHaveAttribute("href", "/song/karunamayane");
    // Metadata links open that value's own page; the title goes to the song itself.
    expect(within(section).getByRole("link", { name: "Yesudas" })).toHaveAttribute("href", "/browse/singer/yesudas");
    expect(within(section).getByRole("link", { name: "Hits" })).toHaveAttribute("href", "/browse/album/hits");
    expect(within(section).getByText("120 views")).toBeInTheDocument();
    expect(within(section).getByText("4.5 (12)")).toBeInTheDocument();
  });

  it("lets a signed-in user hide a section, bring it back, and reorder, remembering it for them only", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me("u1")), "GET /api/v1/home": () => json(200, { generated_at: null, sections: empty }) });
    renderIntl(<HomeSections />);
    await userEvent.click(await screen.findByRole("button", { name: "Customize" }));
    await userEvent.click(screen.getByRole("button", { name: "Hide New songs" }));
    expect(screen.queryByRole("heading", { name: "New songs" })).not.toBeInTheDocument();
    expect(loadPrefs("u1").hidden).toEqual(["new_songs"]);
    expect(loadPrefs("u2").hidden).toEqual([]);

    await userEvent.click(screen.getByRole("button", { name: "Move Popular albums up" }));
    const order = () => screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent).filter((t) => t !== "Hidden sections");
    expect(order().slice(0, 2)).toEqual(["Editors' picks", "Popular albums"]);

    await userEvent.click(screen.getByRole("button", { name: "Show New songs" }));
    expect(screen.getByRole("heading", { name: "New songs" })).toBeInTheDocument();
    expect(loadPrefs("u1").hidden).toEqual([]);
  });

  it("links an album or artist to everything under it, and lets only moderators remove a pick", async () => {
    const item = (id: string, name: string, slug: string) => ({ rank: 1, id, title: title(name), slug, singers: [], albums: [], views: 5, rating_count: 0, comments: 0 });
    const sections = empty.map((s) =>
      s.key === "popular_artists" ? { ...s, items: [item("a1", "Chitra", "chitra")] } : s.key === "new_albums" ? { ...s, items: [item("b1", "Hits", "hits")] } : s.key === "editors_picks" ? { ...s, items: [{ ...item("s1", "Pick", "x"), page: { locale: "en", slug: "pick" } }] } : s,
    );
    const mod = { ...me("m"), roles: ["moderator"] };
    mockFetch({ "GET /api/v1/me": () => json(200, mod), "GET /api/v1/home": () => json(200, { generated_at: null, sections }), "DELETE /api/v1/editors-picks/s1": () => json(200, { picked: false }) });
    renderIntl(<HomeSections />);
    expect(await screen.findByRole("link", { name: "Chitra" })).toHaveAttribute("href", "/browse/artist/chitra");
    expect(screen.getByRole("link", { name: "Hits" })).toHaveAttribute("href", "/browse/album/hits");
    expect(screen.getByRole("link", { name: "Pick" })).toHaveAttribute("href", "/song/pick");
    await userEvent.click(screen.getByRole("button", { name: "Remove Pick from editors' picks" }));
  });

  it("shows no remove button on picks to ordinary users", async () => {
    const sections = empty.map((s) => (s.key === "editors_picks" ? { ...s, items: [{ rank: 1, id: "s1", title: title("Pick"), singers: [], albums: [], views: 0, rating_count: 0, comments: 0 }] } : s));
    mockFetch({ "GET /api/v1/me": () => json(200, me("u1")), "GET /api/v1/home": () => json(200, { generated_at: null, sections }) });
    renderIntl(<HomeSections />);
    await screen.findByText("Pick");
    expect(screen.queryByRole("button", { name: /Remove Pick/ })).not.toBeInTheDocument();
  });

  it("restores a saved layout on the next visit", async () => {
    window.localStorage.setItem("lyrics.home.v1:u1", JSON.stringify({ order: ["new_albums"], hidden: ["popular_songs"] }));
    mockFetch({ "GET /api/v1/me": () => json(200, me("u1")), "GET /api/v1/home": () => json(200, { generated_at: null, sections: empty }) });
    renderIntl(<HomeSections />);
    await screen.findByRole("heading", { name: "New albums" });
    expect(screen.queryByRole("heading", { name: "Most popular songs" })).not.toBeInTheDocument();
    expect(screen.getAllByRole("heading", { level: 3 })[0]).toHaveTextContent("New albums");
    expect(screen.getByText("1 section hidden")).toBeInTheDocument();
  });
});
