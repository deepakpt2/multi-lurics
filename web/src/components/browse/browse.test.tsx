import { describe, expect, it, vi } from "vitest";
import { act, cleanup, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, renderIntl } from "@/test/utils";
import { push } from "@/test/setup";
import { parseState, type BrowseFilter, type BrowseState, type FacetResponse, type SongsResponse, type Suggestion } from "@/lib/browse";
import { BrowseView } from "./browse";
import { FacetList } from "./facet-list";
import { DEBOUNCE_MS, FilterSearch } from "./filter-search";

const refresh = vi.fn();
vi.mock("@/i18n/navigation", async () => {
  const { createElement } = await import("react");
  return {
    Link: ({ href, children, ...rest }: { href: string; children: React.ReactNode; locale?: string }) => {
      delete rest.locale;
      return createElement("a", { href, ...rest }, children);
    },
    useRouter: () => ({ push, refresh }),
    usePathname: () => "/browse",
  };
});

const disp = (text: string) => ({ text, description: null, language: null, script: null, dir: "ltr" as const, fallback: false, resolution: "base" as const });
const sugg = (kind: Suggestion["kind"], slug: string, text: string, over: Partial<Suggestion> = {}): Suggestion => ({
  kind, slug, label: disp(text), matched: text, via: "name", ...over,
});
const wait = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));
const state = (q: string): BrowseState => parseState(new URLSearchParams(q));
const filter = (facet: BrowseFilter["facet"], slug: string, text: string): BrowseFilter => ({ facet, slug, label: disp(text), resolved: true });

describe("filter search", () => {
  const yesu = [
    sugg("singer", "yesudas", "Yesudas"),
    sugg("song", "yesuvinte-namam", "Yesuvinte Namam", { locale: "en" }),
    sugg("album", "yesu-mahathwam", "Yesu Mahathwam"),
  ];

  it("waits for a pause, suggests across kinds, and labels each kind", async () => {
    const { calls } = mockFetch({ "GET /api/v1/browse/suggest": () => json(200, { items: yesu }) });
    renderIntl(<FilterSearch state={state("")} filters={[]} onChange={() => {}} />);
    const box = screen.getByRole("combobox");
    await userEvent.type(box, "y");
    await wait(DEBOUNCE_MS + 50);
    expect(calls).toHaveLength(0);
    await userEvent.type(box, "esu");
    const options = await screen.findAllByRole("option");
    expect(calls).toHaveLength(1);
    expect(new URL(calls[0].url).searchParams.get("q")).toBe("yesu");
    expect(options.map((o) => o.getAttribute("data-kind"))).toEqual(["singer", "song", "album"]);
    expect(within(options[0]).getByText("Singer")).toBeInTheDocument();
    expect(within(options[1]).getByText("Song")).toBeInTheDocument();
    expect(within(options[2]).getByText("Album")).toBeInTheDocument();
  });

  it("choosing a person adds a filter instead of replacing the text, and the box clears for the next one", async () => {
    mockFetch({ "GET /api/v1/browse/suggest": () => json(200, { items: yesu }), "POST /api/v1/search/suggest/accepted": () => new Response(null, { status: 204 }) });
    const onChange = vi.fn();
    renderIntl(<FilterSearch state={state("")} filters={[]} onChange={onChange} />);
    const box = screen.getByRole("combobox");
    await userEvent.type(box, "yesu");
    await userEvent.click(await screen.findByRole("option", { name: /Singer/ }));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ filters: [{ facet: "singer", slug: "yesudas" }], page: 1 }));
    expect(box).toHaveValue("");
    expect(push).not.toHaveBeenCalled();
  });

  it("asks again with the chips already chosen, so suggestions follow the filters", async () => {
    const { calls } = mockFetch({ "GET /api/v1/browse/suggest": () => json(200, { items: [sugg("album", "yesu-mahathwam", "Yesu Mahathwam")] }) });
    renderIntl(<FilterSearch state={state("singer=yesudas&raga=mohanam")} filters={[filter("singer", "yesudas", "Yesudas")]} onChange={() => {}} />);
    await userEvent.type(screen.getByRole("combobox"), "yesu");
    await screen.findByRole("option", { name: /Yesu Mahathwam/ });
    const url = new URL(calls[0].url);
    expect([url.searchParams.getAll("singer"), url.searchParams.getAll("raga"), url.searchParams.get("q")]).toEqual([["yesudas"], ["mohanam"], "yesu"]);
    // Chips show kind and name, and can be removed individually.
    const chips = screen.getByRole("list", { name: "Active filters" });
    expect(within(chips).getByText("Yesudas")).toBeInTheDocument();
    expect(within(chips).getByRole("button", { name: "Remove Singer: Yesudas" })).toBeInTheDocument();
  });

  it("opens a chosen song instead of filtering", async () => {
    mockFetch({ "GET /api/v1/browse/suggest": () => json(200, { items: yesu }), "POST /api/v1/search/suggest/accepted": () => new Response(null, { status: 204 }) });
    const onChange = vi.fn();
    renderIntl(<FilterSearch state={state("")} filters={[]} onChange={onChange} />);
    await userEvent.type(screen.getByRole("combobox"), "yesu");
    await userEvent.click(await screen.findByRole("option", { name: /Song/ }));
    expect(push).toHaveBeenCalledWith("/song/yesuvinte-namam", { locale: "en" });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("works from the keyboard: arrows, Enter, Escape, and Backspace on an empty box", async () => {
    mockFetch({ "GET /api/v1/browse/suggest": () => json(200, { items: yesu }), "POST /api/v1/search/suggest/accepted": () => new Response(null, { status: 204 }) });
    const onChange = vi.fn();
    renderIntl(<FilterSearch state={state("singer=a&album=b")} filters={[]} onChange={onChange} />);
    const box = screen.getByRole("combobox");
    await userEvent.type(box, "yesu");
    await screen.findAllByRole("option");
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{ArrowUp}");
    expect(box).toHaveAttribute("aria-activedescendant", expect.stringMatching(/-o0$/));
    await userEvent.keyboard("{Enter}");
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ filters: [{ facet: "singer", slug: "a" }, { facet: "album", slug: "b" }, { facet: "singer", slug: "yesudas" }] }));
    // Escape closes the list but keeps the text.
    await userEvent.type(box, "yesu");
    await screen.findAllByRole("option");
    await userEvent.keyboard("{Escape}");
    expect(screen.getByRole("listbox", { hidden: true }).closest(".search-popover")).toHaveAttribute("hidden");
    expect(box).toHaveValue("yesu");
    // Backspace removes the last filter only when the box is empty.
    await userEvent.clear(box);
    await userEvent.keyboard("{Backspace}");
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ filters: [{ facet: "singer", slug: "a" }] }));
  });

  it("removes one chip, or all of them", async () => {
    const onChange = vi.fn();
    renderIntl(<FilterSearch state={state("singer=a&album=b")} filters={[filter("singer", "a", "A"), filter("album", "b", "B")]} onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Remove Singer: A" }));
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ filters: [{ facet: "album", slug: "b" }] }));
    await userEvent.click(screen.getByRole("button", { name: "Clear all" }));
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ filters: [], page: 1 }));
  });

  it("says when nothing fits and offers the full search; shows an error with retry", async () => {
    let fail = true;
    mockFetch({ "GET /api/v1/browse/suggest": () => (fail ? json(500, { code: "internal", status: 500, title: "x", type: "x" }) : json(200, { items: [] })) });
    renderIntl(<FilterSearch state={state("")} filters={[]} onChange={() => {}} />);
    await userEvent.type(screen.getByRole("combobox"), "zzz");
    await screen.findByRole("alert");
    fail = false;
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText(/Nothing found for “zzz” with these filters/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Search everything for “zzz”/ })).toHaveAttribute("href", "/search?q=zzz");
  });
});

const song = (title: string, over: Record<string, unknown> = {}) => ({
  page: { locale: "en", slug: title.toLowerCase().replace(/ /g, "-") },
  title: disp(title), year: 1980,
  singers: [{ slug: "yesudas", name: disp("Yesudas") }],
  albums: [{ slug: "yesu-mahathwam", name: disp("Yesu Mahathwam") }],
  original_language: { code: "ml", name: "മലയാളം" },
  translations: [{ code: "en", name: "English" }],
  ...over,
});
const songs = (over: Partial<SongsResponse> = {}): SongsResponse =>
  ({ items: [song("Karunamayane")], page: 1, page_size: 20, total: 1, total_pages: 1, filters: [], ...over }) as SongsResponse;

describe("browse page", () => {
  it("lists songs with title, singer, album, language and translations, linking to the song page", () => {
    renderIntl(<BrowseView state={state("")} result={{ ok: true, data: songs() }} />);
    expect(screen.getByRole("link", { name: /Karunamayane/ })).toHaveAttribute("href", "/song/karunamayane");
    const card = within(screen.getByRole("list", { name: "Songs" })).getByRole("listitem");
    expect(within(card).getByRole("link", { name: "Yesudas" })).toHaveAttribute("href", "/browse/singer/yesudas");
    expect(within(card).getByRole("link", { name: "Yesu Mahathwam" })).toHaveAttribute("href", "/browse/album/yesu-mahathwam");
    expect(within(card).getByText("മലയാളം")).toBeInTheDocument();
    expect(within(card).getByText("English")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "1 song" })).toBeInTheDocument();
    // Links to every metadata page, singular routes.
    for (const [name, href] of [["Albums", "/browse/album"], ["Artists", "/browse/artist"], ["Singers", "/browse/singer"], ["Lyricists", "/browse/lyricist"], ["Composers", "/browse/composer"]]) {
      expect(within(screen.getByRole("navigation", { name: "Browse by" })).getByRole("link", { name })).toHaveAttribute("href", href);
    }
  });

  it("a person on a card opens that person's own page, whatever filters are chosen", () => {
    renderIntl(<BrowseView state={state("album=yesu-mahathwam")} result={{ ok: true, data: songs({ filters: [filter("album", "yesu-mahathwam", "Yesu Mahathwam")] }) }} />);
    expect(within(within(screen.getByRole("list", { name: "Songs" })).getByRole("listitem")).getByRole("link", { name: "Yesudas" })).toHaveAttribute("href", "/browse/singer/yesudas");
  });

  it("pages keep the filters, mark the current page, and the size and sort live in the address", async () => {
    renderIntl(<BrowseView state={state("singer=yesudas&page=2")} result={{ ok: true, data: songs({ page: 2, total: 41, total_pages: 3 }) }} />);
    const nav = screen.getByRole("navigation", { name: "Pagination" });
    expect(within(nav).getByRole("link", { name: "Page 3" })).toHaveAttribute("href", "/browse?singer=yesudas&page=3");
    expect(within(nav).getByRole("link", { name: "Previous" })).toHaveAttribute("href", "/browse?singer=yesudas");
    expect(within(nav).getByRole("link", { name: "Page 2" })).toHaveAttribute("aria-current", "page");
    await userEvent.selectOptions(screen.getByLabelText("Per page"), "50");
    expect(push).toHaveBeenLastCalledWith("/browse?singer=yesudas&page_size=50");
    await userEvent.selectOptions(screen.getByLabelText("Sort by"), "title");
    expect(push).toHaveBeenLastCalledWith("/browse?singer=yesudas&sort=title");
  });

  it("shows empty, no-match, unresolved and error states", async () => {
    const show = (ui: React.ReactElement) => {
      cleanup();
      renderIntl(ui);
    };
    show(<BrowseView state={state("")} result={{ ok: true, data: songs({ items: [], total: 0, total_pages: 0 }) }} />);
    expect(screen.getByText("There are no songs to browse yet.")).toBeInTheDocument();
    show(<BrowseView state={state("singer=nobody")} result={{ ok: true, data: songs({ items: [], total: 0, total_pages: 0, filters: [{ ...filter("singer", "nobody", "nobody"), resolved: false }] }) }} />);
    expect(screen.getByText("No songs match these filters.")).toBeInTheDocument();
    expect(screen.getByText(/“nobody” was not found/)).toBeInTheDocument();
    show(<BrowseView state={state("")} result={{ ok: false, status: 500 }} />);
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(refresh).toHaveBeenCalled();
  });
});

describe("metadata page", () => {
  const facet = (over: Partial<FacetResponse> = {}): FacetResponse =>
    ({ facet: "singer", items: [{ slug: "yesudas", label: disp("Yesudas"), songs: 2 }], page: 1, page_size: 20, total: 1, total_pages: 1, filters: [], ...over }) as FacetResponse;

  it("lists values; each opens the songs that have it, on top of the chosen filters", () => {
    renderIntl(<FacetList facet="singer" state={state("album=x")} q="" result={{ ok: true, data: facet() }} />);
    expect(screen.getByRole("heading", { name: /Browse Singers/ })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Yesudas" })).toHaveAttribute("href", "/browse?album=x&singer=yesudas");
    expect(screen.getByText("2 songs")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "All songs" })).toHaveAttribute("href", "/browse?album=x");
  });

  it("translates religion codes, keeps the search text across pages, and has an empty state", () => {
    renderIntl(
      <FacetList facet="religion" state={state("")} q="chr" result={{ ok: true, data: facet({ facet: "religion", items: [{ slug: "christian", label: disp("christian"), songs: 1 }], total: 40, total_pages: 2 }) }} />,
    );
    expect(screen.getByRole("link", { name: "Christian" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Page 2" })).toHaveAttribute("href", "/browse/religion?page=2&q=chr");
    cleanup();
    renderIntl(<FacetList facet="religion" state={state("")} q="zzz" result={{ ok: true, data: facet({ items: [], total: 0, total_pages: 0 }) }} />);
    expect(screen.getByText("Nothing matches “zzz”.")).toBeInTheDocument();
  });
});
