import { describe, expect, it } from "vitest";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { parsePrefill } from "@/lib/contribute";
import { ContributeSong } from "./contribute-song";

const me = { id: "u1", email: "u@example.com", display_name: "U", roles: [], email_verified: true };
const lang = (iso: string, name: string, endonym: string, script: string) => ({ id: iso, iso639_3: iso, iso639_1: null, name_en: name, endonym, is_rtl: false, default_script: script });
const page = (items: unknown[]) => ({ items, next_cursor: null });
const suggestion = (id: string, text: string) => ({ id, type: "album", display: { text, dir: "ltr", resolution: "primary" }, localized_names: [], matched_via: "name", related: [], score: 1, detail: null });

const routes = (extra: Record<string, () => Response> = {}) => ({
  "GET /api/v1/me": () => json(200, me),
  "GET /api/v1/languages": () => json(200, page([lang("mal", "Malayalam", "മലയാളം", "Mlym"), lang("eng", "English", "English", "Latn")])),
  "GET /api/v1/scripts": () => json(200, page([{ id: "s1", iso15924: "Mlym", name_en: "Malayalam", is_rtl: false }, { id: "s2", iso15924: "Latn", name_en: "Latin", is_rtl: false }])),
  "GET /api/v1/search": () => json(200, { groups: [{ type: "compositions", items: [{ id: "c1", type: "composition", display: { text: "Kannum Kannum Existing", dir: "ltr", resolution: "primary" }, match: { text: "x", kind: "prefix", via: "name" }, year: 1989 }] }] }),
  "GET /api/v1/entities/suggest": () => json(200, { items: [suggestion("al1", "Kireedam")] }),
  ...extra,
});

describe("contribute a song", () => {
  it("asks a signed-out visitor to sign in", async () => {
    mockFetch({ ...routes(), "GET /api/v1/me": () => problem(401, "unauthenticated") });
    renderIntl(<ContributeSong prefill={parsePrefill({})} />);
    expect(await screen.findByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/login");
  });

  it("takes details and a transliteration with no original, and suggests existing songs and albums while typing", async () => {
    const { calls } = mockFetch(routes({ "POST /api/v1/contribute/song": () => json(201, { composition_id: "c9", recording_id: "r9", statuses: ["pending"] }) }));
    renderIntl(<ContributeSong prefill={parsePrefill({})} />);
    await userEvent.type(await screen.findByLabelText("Song title"), "Kannum Kannum");
    // Similar songs already in the catalogue are shown so the same song is not added twice.
    expect(await screen.findByText("Songs that look similar are already here:")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Kannum Kannum Existing" })).toBeInTheDocument();

    // Albums are suggested; choosing one uses the existing record.
    await userEvent.type(screen.getByLabelText("Album or film"), "Kire");
    await userEvent.click(await screen.findByRole("button", { name: "Kireedam" }));
    // Singers: a new name is added as new.
    await userEvent.type(screen.getByLabelText("Singers"), "Somebody New{Enter}");
    expect(screen.getByText("Somebody New")).toBeInTheDocument();

    await userEvent.selectOptions(screen.getByLabelText("Language"), "mal");
    await userEvent.click(screen.getByRole("radio", { name: "Transliteration" }));
    expect(screen.getByText(/Someone else can add the original later/)).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Lyrics"), "kannum kannum");
    await userEvent.click(screen.getByRole("button", { name: "Submit" }));

    expect(await screen.findByText("Thank you!")).toBeInTheDocument();
    expect(screen.getByText(/once a moderator has checked them/)).toBeInTheDocument();
    const sent = (await calls.find((c) => c.method === "POST")!.json()) as Record<string, unknown>;
    expect(sent).toMatchObject({
      title: "Kannum Kannum",
      album: { id: "al1", name: "Kireedam" },
      singers: [{ id: null, name: "Somebody New" }],
      lyrics: [{ type: "transliteration", language: "mal", script: "Mlym", body: "kannum kannum" }],
    });
  });

  it("another language adds a section; the song can have several", async () => {
    mockFetch(routes());
    renderIntl(<ContributeSong prefill={parsePrefill({})} />);
    await screen.findByLabelText("Song title");
    await userEvent.click(screen.getByRole("button", { name: "Add the same song in another language" }));
    expect(screen.getAllByLabelText("Lyrics")).toHaveLength(2);
    expect(screen.getByText("Lyrics in another language (2)")).toBeInTheDocument();
    // The second section starts as a translation, since there is already an original.
    const second = screen.getByText("Lyrics in another language (2)").closest("fieldset")!;
    expect(within(second).getByRole("radio", { name: "Translation" })).toBeChecked();
    await userEvent.click(within(second).getByRole("button", { name: "Remove this language" }));
    expect(screen.getAllByLabelText("Lyrics")).toHaveLength(1);
  });

  it("from an album page the album is filled in and cannot be changed", async () => {
    mockFetch(routes());
    renderIntl(<ContributeSong prefill={parsePrefill({ album: "al1", album_name: "Kireedam" })} />);
    await screen.findByLabelText("Song title");
    expect(screen.getByText("Kireedam")).toBeInTheDocument();
    expect(screen.getByText("fixed")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove Kireedam" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Album or film")).not.toBeInTheDocument();
  });

  it("a devotional prefill (deity page) fixes the kind and the deity", async () => {
    mockFetch(routes());
    renderIntl(<ContributeSong prefill={parsePrefill({ deity: "Hanuman" })} />);
    expect(await screen.findByLabelText("Kind of song")).toBeDisabled();
    expect(screen.getByText("Hanuman")).toBeInTheDocument();
    expect(screen.getByLabelText("Form")).toBeInTheDocument();
  });

  it("needs a title, some lyrics and a language, and offers 'add anyway' for a duplicate on the album", async () => {
    let allow: unknown;
    const { calls } = mockFetch(routes({ "POST /api/v1/contribute/song": () => problem(409, "possible_duplicate", { candidates: ["c1"] }) }));
    renderIntl(<ContributeSong prefill={parsePrefill({})} />);
    await userEvent.click(await screen.findByRole("button", { name: "Submit" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Please enter the song title.");
    await userEvent.type(screen.getByLabelText("Song title"), "T");
    await userEvent.type(screen.getByLabelText("Lyrics"), "x");
    await userEvent.click(screen.getByRole("button", { name: "Submit" }));
    expect(screen.getByRole("alert")).toHaveTextContent("choose the language");
    await userEvent.selectOptions(screen.getByLabelText("Language"), "eng");
    await userEvent.click(screen.getByRole("button", { name: "Submit" }));
    expect(await screen.findByText("A song with this title is already on this album.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Add it anyway" }));
    allow = await calls.filter((c) => c.method === "POST").at(-1)!.json();
    expect(allow).toMatchObject({ allow_duplicate: true });
  });
});
