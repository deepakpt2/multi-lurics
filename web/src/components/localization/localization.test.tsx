import { afterEach, describe, expect, it } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { setDisplayLanguage } from "@/api/client";
import { CompositionDetail } from "../catalogue/compositions";
import { ArtistDetail } from "../catalogue/artists";
import { Name } from "./localized";
import { LocalizationForm, NamesView } from "./names";
import { ModerationQueue } from "./moderation";

afterEach(() => setDisplayLanguage(undefined));

const T = "2026-01-01T00:00:00Z";
const disp = (text: string, over: Record<string, unknown> = {}) => ({
  text, description: null, language: "en", script: null, dir: "ltr", fallback: false, resolution: "requested", ...over,
});

const languages = {
  items: [
    { id: "l1", iso639_3: "mal", iso639_1: "ml", name_en: "Malayalam", endonym: "മലയാളം", is_rtl: false, default_script: "Mlym", is_extension: false },
    { id: "l2", iso639_3: "ara", iso639_1: "ar", name_en: "Arabic", endonym: "العربية", is_rtl: true, default_script: "Arab", is_extension: false },
    { id: "l3", iso639_3: "eng", iso639_1: "en", name_en: "English", endonym: "English", is_rtl: false, default_script: "Latn", is_extension: false },
  ],
  next_cursor: null,
};
const scripts = {
  items: [
    { id: "s1", iso15924: "Arab", numeric_code: 160, name_en: "Arabic", is_rtl: true, is_extension: false },
    { id: "s2", iso15924: "Latn", numeric_code: 215, name_en: "Latin", is_rtl: false, is_extension: false },
  ],
  next_cursor: null,
};

describe("Name (display with fallback label)", () => {
  it("renders Malayalam inside an Arabic page with its own lang and direction, unlabelled when requested", () => {
    renderIntl(<Name display={disp("കാറ്റിൻ മൊഴി", { language: "ml" }) as never} />, "ar");
    const el = screen.getByText("കാറ്റിൻ മൊഴി");
    expect(el).toHaveAttribute("lang", "ml");
    expect(el).toHaveAttribute("dir", "ltr");
    expect(el).toHaveAttribute("translate", "no");
    expect(document.querySelector("[data-fallback]")).toBeNull();
  });

  it("renders Arabic with mixed Latin text right-to-left and keeps bidi marks", () => {
    const text = "أغنية ⁧Love Story 2024⁩ الحب‏";
    renderIntl(<Name display={disp(text, { language: "ar", dir: "rtl" }) as never} />, "en");
    const el = screen.getByText((c) => c === text);
    expect(el).toHaveAttribute("dir", "rtl");
    expect(el).toHaveAttribute("lang", "ar");
    expect(el.textContent).toBe(text);
  });

  it("labels text that is not in the reader's language, and base names of unknown language", () => {
    renderIntl(
      <>
        <Name display={disp("Language of the Wind", { fallback: true, resolution: "fallback" }) as never} />
        <Name display={disp("Kaatrin Mozhi", { language: null, dir: "auto", fallback: true, resolution: "base" }) as never} />
      </>,
      "ar",
    );
    const badges = [...document.querySelectorAll("[data-fallback]")].map((b) => b.textContent);
    expect(badges).toEqual(["معروض بلغة الإنجليزية", "الاسم الأصلي"]);
  });
});

describe("credits are grouped by category", () => {
  it("never mixes original songwriters, translators and adapters", async () => {
    const credit = (artist: string, role: string, category: string, id: string) => ({
      artist_id: id, artist_name: artist, artist_display: disp(artist), role, role_id: `r-${role}`, role_category: category, role_display: disp(role),
    });
    mockFetch({
      "GET /api/v1/compositions/c1": () =>
        json(200, {
          id: "c1", title: "Song", display: disp("Song"), iswc: null, year: null, version: 1, created_by: "u", created_at: T, updated_at: T,
          credits: [
            credit("Adapter Person", "adapter", "adapter", "a3"),
            credit("Writer", "composer", "original_songwriter", "a1"),
            credit("Translator Person", "translator", "translator", "a2"),
            credit("Writer", "lyricist", "original_songwriter", "a1"),
          ],
          recordings: [], recording_count: 0,
        }),
    });
    renderIntl(<CompositionDetail id="c1" />);
    await screen.findByRole("heading", { level: 1, name: "Song" });
    const groups = [...document.querySelectorAll("[data-category]")].map((g) => [g.getAttribute("data-category"), within(g as HTMLElement).getAllByRole("listitem").length]);
    expect(groups).toEqual([["original_songwriter", 2], ["translator", 1], ["adapter", 1]]);
    expect(screen.getByRole("heading", { name: "Translators" })).toBeInTheDocument();
    expect(within(document.querySelector('[data-category="translator"]') as HTMLElement).getByText("Translator Person")).toBeInTheDocument();
  });

  it("one artist, many names: the same id is shown in the reader's language", async () => {
    mockFetch({
      "GET /api/v1/artists/a1": () =>
        json(200, {
          id: "a1", name: "Yesudas", kind: "person", display: disp("يسوداس", { language: "ar-Arab", script: "Arab", dir: "rtl" }),
          disambiguation: null, isni: null, version: 1, created_by: "u", created_at: T, updated_at: T, credits: [],
        }),
    });
    renderIntl(<ArtistDetail id="a1" />, "ar");
    const h = await screen.findByRole("heading", { level: 1 });
    expect(within(h).getByText("يسوداس")).toHaveAttribute("dir", "rtl");
  });
});

describe("language-aware localization form", () => {
  const refs = { "GET /api/v1/languages": () => json(200, languages), "GET /api/v1/scripts": () => json(200, scripts) };

  it("types right-to-left for Arabic, lets the script override, and submits codes", async () => {
    const { calls } = mockFetch({
      ...refs,
      "POST /api/v1/entities/composition/c1/localizations": () =>
        json(201, { id: "n1", verification_state: "pending", current_revision_id: "rev1" }),
    });
    const onSaved = () => {};
    renderIntl(<LocalizationForm entityType="composition" entityId="c1" onSaved={onSaved} />);
    const language = await screen.findByLabelText("Language");
    const name = screen.getByLabelText("Name");
    expect(name).toHaveAttribute("dir", "ltr");
    await userEvent.selectOptions(language, "ara");
    expect(name).toHaveAttribute("dir", "rtl");
    expect(name).toHaveAttribute("lang", "ar");
    await userEvent.selectOptions(screen.getByLabelText("Script"), "Latn");
    expect(name).toHaveAttribute("dir", "ltr");
    await userEvent.selectOptions(screen.getByLabelText("Script"), "Arab");
    expect(name).toHaveAttribute("dir", "rtl");

    await userEvent.type(name, "لغة الريح");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Thanks. Your suggestion is waiting for review.")).toBeInTheDocument();
    const sent = JSON.parse(await calls.at(-1)!.clone().text());
    expect(sent).toMatchObject({ language: "ara", script: "Arab", name: "لغة الريح", provenance: "original", source: null });
  });

  it("artists: no machine provenance, source hint, and server errors are localized", async () => {
    mockFetch({
      ...refs,
      "POST /api/v1/entities/artist/a1/localizations": () =>
        problem(422, "validation_failed", { errors: [{ field: "source", code: "required" }] }),
    });
    renderIntl(<LocalizationForm entityType="artist" entityId="a1" onSaved={() => {}} />);
    await userEvent.selectOptions(await screen.findByLabelText("Language"), "mal");
    expect(screen.queryByRole("option", { name: "Machine" })).not.toBeInTheDocument();
    expect(screen.getByText(/never machine-translated/)).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Name"), "യേശുദാസ്");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("This field is required.")).toBeInTheDocument();
  });

  it("slot conflicts and unauthorized submissions show their own messages, not a generic error", async () => {
    let n = 0;
    mockFetch({
      ...refs,
      "POST /api/v1/entities/composition/c1/localizations": () =>
        n++ === 0 ? problem(401, "unauthenticated") : problem(409, "localization_slot_taken", { existing_id: "n9" }),
    });
    renderIntl(<LocalizationForm entityType="composition" entityId="c1" onSaved={() => {}} />);
    await userEvent.selectOptions(await screen.findByLabelText("Language"), "eng");
    await userEvent.type(screen.getByLabelText("Name"), "X");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Please sign in to continue.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/already have a name/)).toBeInTheDocument();
  });

  it("lists verified names and aliases with their sources, each in its own direction", async () => {
    mockFetch({
      ...refs,
      "GET /api/v1/entities/artist/a1/localizations": () =>
        json(200, { items: [{ id: "n1", language: "ar", script: null, dir: "rtl", name: "يسوداس", is_preferred: true, source: "Official site" }], next_cursor: null }),
      "GET /api/v1/entities/artist/a1/aliases": () =>
        json(200, { items: [{ id: "al1", alias: "Dasettan", language: "ml", dir: "ltr", source: "Interview, 2019" }], next_cursor: null }),
    });
    renderIntl(<NamesView entityType="artist" entityId="a1" />);
    expect(await screen.findByText("يسوداس")).toHaveAttribute("dir", "rtl");
    expect(screen.getByText("Dasettan")).toHaveAttribute("lang", "ml");
    expect(screen.getByText(/source: Interview, 2019/)).toBeInTheDocument();
  });
});

describe("moderation queue", () => {
  const item = { id: "n1", entity_type: "composition", entity_id: "c1", language: "ar", script: null, dir: "rtl", name: "لغة الريح", provenance: "community", source: null, current_revision_id: "rev1" };

  it("approves with the revision it was shown, and removes the item", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/moderation/localizations": () => json(200, { items: [item], next_cursor: null }),
      "POST /api/v1/localizations/n1/verify": () => json(200, { ...item, verification_state: "verified" }),
    });
    renderIntl(<ModerationQueue />);
    expect(await screen.findByText("لغة الريح")).toHaveAttribute("dir", "rtl");
    await userEvent.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(screen.getByText("Nothing is waiting for review.")).toBeInTheDocument());
    expect(JSON.parse(await calls.at(-1)!.clone().text())).toEqual({ expected_parent_revision_id: "rev1" });
  });

  it("a stale decision shows the conflict notice instead of overwriting", async () => {
    mockFetch({
      "GET /api/v1/moderation/localizations": () => json(200, { items: [item], next_cursor: null }),
      "POST /api/v1/localizations/n1/reject": () => problem(409, "stale_revision", { current_revision_id: "rev2" }),
    });
    renderIntl(<ModerationQueue />);
    await userEvent.click(await screen.findByRole("button", { name: "Reject" }));
    expect(await screen.findByText("Someone else changed this")).toBeInTheDocument();
    expect(screen.getByText("لغة الريح")).toBeInTheDocument();
  });

  it("non-moderators get a permission error; an empty queue says so", async () => {
    mockFetch({ "GET /api/v1/moderation/localizations": () => problem(403, "forbidden") });
    renderIntl(<ModerationQueue />);
    expect(await screen.findByText("You do not have permission to do this.")).toBeInTheDocument();
  });
});

describe("display language is sent with reads", () => {
  it("adds lang to API GETs from the UI locale, and never overrides an explicit one", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/artists/a1": () => json(200, { id: "a1", name: "x", display: disp("x"), kind: "person", credits: [], version: 1, created_by: "u", created_at: T, updated_at: T }),
    });
    setDisplayLanguage("ar");
    renderIntl(<ArtistDetail id="a1" />, "ar");
    await screen.findByRole("heading", { level: 1 });
    expect(new URL(calls[0].url).searchParams.get("lang")).toBe("ar");
  });
});
