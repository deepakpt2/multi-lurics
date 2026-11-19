import { describe, expect, it } from "vitest";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { push } from "@/test/setup";
import { DEBOUNCE_MS, SearchBox, SearchResults } from "./search";

const disp = (text: string, over: Record<string, unknown> = {}) => ({
  text, description: null, language: null, script: null, dir: "ltr", fallback: false, resolution: "base", ...over,
});
const item = (type: string, id: string, text: string, over: Record<string, unknown> = {}) => ({
  type, id, display: disp(text), match: { text, kind: "exact", via: "name" }, sub_kind: null, detail: null, year: null, parent: null, ...over,
});
const wait = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));

describe("typeahead", () => {
  it("waits for a pause, sends one request for the final text, and does not search below the minimum", async () => {
    const { calls } = mockFetch({ "GET /api/v1/search/suggest": () => json(200, { items: [item("artist", "a1", "A. R. Rahman")] }) });
    renderIntl(<SearchBox />);
    const box = screen.getByRole("combobox", { name: "Search lyrics, songs, artists and albums" });
    await userEvent.type(box, "r");
    await wait(DEBOUNCE_MS + 50);
    expect(calls).toHaveLength(0);
    await userEvent.type(box, "ah");
    expect(calls).toHaveLength(0);
    expect(await screen.findByRole("option", { name: /A\. R\. Rahman/ })).toBeInTheDocument();
    expect(calls).toHaveLength(1);
    expect(new URL(calls[0].url).searchParams.get("q")).toBe("rah");
  });

  it("cancels the stale request: a slow earlier answer never replaces the newer one", async () => {
    let first = true;
    const { calls } = mockFetch({
      "GET /api/v1/search/suggest": () => {
        const q = new URL(calls.at(-1)!.url).searchParams.get("q");
        if (first) {
          first = false;
          return new Response(JSON.stringify({ items: [item("artist", "old", "Stale Result")] }), { headers: { "content-type": "application/json" } });
        }
        return json(200, { items: [item("artist", "new", `Fresh ${q}`)] });
      },
    });
    renderIntl(<SearchBox />);
    const box = screen.getByRole("combobox");
    await userEvent.type(box, "ab");
    await wait(DEBOUNCE_MS + 10);
    await userEvent.type(box, "c");
    expect(await screen.findByRole("option", { name: /Fresh abc/ })).toBeInTheDocument();
    expect(screen.queryByText("Stale Result")).not.toBeInTheDocument();
  });

  it("navigates with the keyboard, reports acceptance, and opens the chosen entity", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/search/suggest": () => json(200, { items: [item("composition", "c1", "Kaatrin Mozhi"), item("artist", "a1", "Kaatrin Band")] }),
      "POST /api/v1/search/suggest/accepted": () => new Response(null, { status: 204 }),
    });
    renderIntl(<SearchBox />);
    const box = screen.getByRole("combobox");
    await userEvent.type(box, "kaa");
    await screen.findAllByRole("option");
    await userEvent.keyboard("{ArrowDown}{ArrowDown}");
    expect(box).toHaveAttribute("aria-activedescendant", screen.getAllByRole("option")[1].id);
    await userEvent.keyboard("{Enter}");
    expect(push).toHaveBeenCalledWith("/artists/a1");
    await waitFor(() => expect(calls.some((c) => c.method === "POST" && c.url.endsWith("/accepted"))).toBe(true));
    expect(calls.find((c) => c.method === "POST")!.headers.get("x-requested-with")).toBe("fetch");
  });

  it("Enter without a highlighted suggestion runs the full search; Escape clears", async () => {
    mockFetch({ "GET /api/v1/search/suggest": () => json(200, { items: [item("artist", "a1", "Alpha")] }) });
    renderIntl(<SearchBox />);
    const box = screen.getByRole("combobox");
    await userEvent.type(box, "al pha");
    await screen.findByRole("option");
    await userEvent.keyboard("{Enter}");
    expect(push).toHaveBeenCalledWith("/search?q=al%20pha");
    await userEvent.keyboard("{Escape}");
    expect(box).toHaveValue("");
  });

  it("shows no-results, a rate-limit error with retry, and says what matched", async () => {
    let n = 0;
    mockFetch({
      "GET /api/v1/search/suggest": () => {
        n += 1;
        if (n === 1) return problem(429, "rate_limited");
        if (n === 2) return json(200, { items: [] });
        return json(200, { items: [item("composition", "c1", "കാറ്റിൻ മൊഴി", { match: { text: "Kaatrin Mozhi", kind: "exact", via: "transliteration" } })] });
      },
    });
    renderIntl(<SearchBox />);
    await userEvent.type(screen.getByRole("combobox"), "kaa");
    expect(await screen.findByText("You are searching too quickly. Please wait a moment and try again.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Nothing found for “kaa”.", { exact: false })).toBeInTheDocument();
    await userEvent.type(screen.getByRole("combobox"), "x");
    const option = await screen.findByRole("option");
    expect(within(option).getByText("Transliteration")).toBeInTheDocument();
    expect(within(option).getByText("Kaatrin Mozhi")).toBeInTheDocument();
    expect(within(option).getByText("കാറ്റിൻ മൊഴി")).toHaveAttribute("translate", "no");
  });
});

describe("results page", () => {
  const groups = () => [
    { type: "compositions", items: [item("composition", "c1", "Kaatrin Mozhi")], next_cursor: null },
    { type: "recordings", items: [item("recording", "r1", "Take 1", { parent: { id: "c1", title: "Kaatrin Mozhi" } })], next_cursor: null },
    { type: "artists", items: [item("artist", "a1", "Mozhi Band", { sub_kind: "group" })], next_cursor: "cur1" },
    { type: "albums", items: [], next_cursor: null },
    { type: "contributors", items: [], next_cursor: null },
  ];

  it("renders typed groups with links, hides empty groups, and pages one group by cursor", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/search": () => {
        const p = new URL(calls.at(-1)!.url).searchParams;
        return p.get("cursor")
          ? json(200, { groups: [{ type: "artists", items: [item("artist", "a2", "Second Band")], next_cursor: null }] })
          : json(200, { groups: groups() });
      },
    });
    renderIntl(<SearchResults initialQuery="mozhi" />);
    expect(screen.getByRole("status")).toHaveTextContent("Loading…");
    const songs = (await screen.findByRole("heading", { name: "Songs" })).closest("section")!;
    expect(within(songs).getByRole("link", { name: "Kaatrin Mozhi" })).toHaveAttribute("href", "/compositions/c1");
    expect(screen.getByRole("heading", { name: "Recordings" })).toBeInTheDocument();
    expect(screen.getByText("Recording of Kaatrin Mozhi", { exact: false })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Albums" })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Load more" }));
    expect(await screen.findByRole("link", { name: "Second Band" })).toHaveAttribute("href", "/artists/a2");
    const last = new URL(calls.at(-1)!.url).searchParams;
    expect([last.get("type"), last.get("cursor")]).toEqual(["artists", "cur1"]);
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Mozhi Band" })).toBeInTheDocument();
  });

  it("empty and failure states; the form searches again", async () => {
    let n = 0;
    mockFetch({
      "GET /api/v1/search": () => (n++ === 0 ? problem(503, "unavailable") : json(200, { groups: [{ type: "compositions", items: [], next_cursor: null }] })),
    });
    renderIntl(<SearchResults initialQuery="zzz" />);
    await userEvent.click(await screen.findByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Nothing found for “zzz”.")).toBeInTheDocument();
    const field = screen.getByLabelText("Search lyrics, songs, artists and albums");
    await userEvent.clear(field);
    await userEvent.type(field, "new{Enter}");
    expect(push).toHaveBeenCalledWith("/search?q=new");
  });

  it("no query asks for one without calling the server; Arabic renders", async () => {
    const { calls } = mockFetch({});
    renderIntl(<SearchResults initialQuery="" />, "ar");
    expect(screen.getByText("اكتب عنوانًا أو اسمًا أو تهجئة للبحث.")).toBeInTheDocument();
    expect(calls).toHaveLength(0);
  });
});
