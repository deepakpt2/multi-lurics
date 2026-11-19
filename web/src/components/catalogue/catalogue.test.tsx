import { describe, expect, it } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { push } from "@/test/setup";
import { ArtistDetail, ArtistEdit, ArtistList } from "./artists";
import { CompositionForm } from "./compositions";
import { RecordingNew } from "./recordings";

const T = "2026-01-01T00:00:00Z";
const disp = (text: string, over: Record<string, unknown> = {}) => ({
  text, description: null, language: "en", script: null, dir: "ltr", fallback: false, resolution: "requested", ...over,
});
const artist = (over: Record<string, unknown> = {}) => {
  const a = {
    id: "a1", name: "فيروز", kind: "person", disambiguation: null, isni: null, version: 1,
    created_by: "u1", created_at: T, updated_at: T, credits: [], ...over,
  };
  return { display: disp(String(a.name)), ...a };
};
const composition = { id: "c1", title: "Kaatrin Mozhi", iswc: null, year: null, version: 1, created_by: "u1", created_at: T, updated_at: T, credits: [], recordings: [], recording_count: 0 };
const recording = {
  id: "r1", composition_id: "c1", title: "Original take", isrc: "USRC17607839", duration_ms: 245000, year: null, version: 1,
  created_by: "u1", created_at: T, updated_at: T, composition: { id: "c1", title: "Kaatrin Mozhi" },
  credits: [{ artist_id: "a1", artist_name: "Singer", role: "performer" }], relationships: [], related_from: [], albums: [],
};

async function body(req: Request) {
  return JSON.parse(await req.clone().text());
}

describe("recording create", () => {
  it("adds a cover of an existing recording: same composition, nothing copied", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/compositions/c1": () => json(200, composition),
      "GET /api/v1/recordings/r1": () => json(200, recording),
      "POST /api/v1/recordings": () => json(201, { ...recording, id: "r2" }),
    });
    renderIntl(<RecordingNew query={{ composition_id: "c1", related: "r1", kind: "cover" }} />);
    expect(await screen.findByText("Kaatrin Mozhi")).toHaveAttribute("translate", "no");
    const relationships = screen.getByRole("group", { name: "Related recordings" });
    expect(within(relationships).getByText("Original take")).toBeInTheDocument();
    expect(within(relationships).getByRole("combobox", { name: "Choice for Original take" })).toHaveValue("cover");

    await userEvent.type(screen.getByLabelText("Title"), "Cover take");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(push).toHaveBeenCalledWith("/recordings/r2"));
    const sent = await body(calls.at(-1)!);
    expect(sent).toMatchObject({
      composition_id: "c1",
      title: "Cover take",
      isrc: null,
      duration_ms: null,
      credits: [],
      relationships: [{ kind: "cover", related_recording_id: "r1" }],
    });
    expect(calls.at(-1)!.headers.get("x-requested-with")).toBe("fetch");
  });

  it("shows the server's composition mismatch on the relationships group", async () => {
    mockFetch({
      "GET /api/v1/compositions/c1": () => json(200, composition),
      "GET /api/v1/recordings/r1": () => json(200, recording),
      "POST /api/v1/recordings": () =>
        problem(422, "validation_failed", { errors: [{ field: "relationships", code: "composition_mismatch" }] }),
    });
    renderIntl(<RecordingNew query={{ composition_id: "c1", related: "r1" }} />);
    await userEvent.type(await screen.findByLabelText("Title"), "X");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    const group = screen.getByRole("group", { name: "Related recordings" });
    expect(await within(group).findByText("This relationship requires both recordings to belong to the same composition.")).toBeInTheDocument();
    expect(group).toHaveAttribute("aria-invalid", "true");
    expect(push).not.toHaveBeenCalled();
  });
});

describe("composition form", () => {
  it("records composer and lyricist as two distinct credits for one artist", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/artists": () => json(200, { items: [artist({ name: "Ilaiyaraaja" })], next_cursor: null }),
      "POST /api/v1/compositions": () => json(201, { ...composition, id: "c2" }),
    });
    renderIntl(<CompositionForm />);
    await userEvent.type(screen.getByLabelText("Title"), "Work");
    for (const role of ["composer", "lyricist"]) {
      await userEvent.type(screen.getByLabelText("Find an artist"), "Ila");
      await userEvent.click(screen.getByRole("button", { name: "Find" }));
      await userEvent.click(await screen.findByRole("button", { name: "Ilaiyaraaja" }));
      const selects = screen.getAllByRole("combobox", { name: "Choice for Ilaiyaraaja" });
      await userEvent.selectOptions(selects.at(-1)!, role);
    }
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(push).toHaveBeenCalledWith("/compositions/c2"));
    expect((await body(calls.at(-1)!)).credits).toEqual([
      { artist_id: "a1", role: "composer" },
      { artist_id: "a1", role: "lyricist" },
    ]);
  });

  it("duplicate identifier links to the existing record and is not shown as an edit conflict", async () => {
    mockFetch({
      "POST /api/v1/compositions": () =>
        problem(409, "duplicate_identifier", { existing_id: "c9", errors: [{ field: "iswc", code: "duplicate" }] }),
    });
    renderIntl(<CompositionForm />);
    await userEvent.type(screen.getByLabelText("Title"), "Work");
    await userEvent.type(screen.getByLabelText("ISWC"), "T-034.524.680-1");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("link", { name: "View the existing record" })).toHaveAttribute("href", "/compositions/c9");
    expect(screen.getByText("This identifier already belongs to another record.")).toBeInTheDocument();
    expect(screen.queryByText("Someone else changed this")).not.toBeInTheDocument();
    expect(screen.getByLabelText("ISWC")).toHaveAttribute("aria-invalid", "true");
  });

  it("unauthorized: an anonymous save asks to sign in and does not navigate", async () => {
    mockFetch({ "POST /api/v1/compositions": () => problem(401, "unauthenticated") });
    renderIntl(<CompositionForm />);
    await userEvent.type(screen.getByLabelText("Title"), "Work");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Please sign in to continue.")).toBeInTheDocument();
    expect(push).not.toHaveBeenCalled();
  });
});

describe("artist edit", () => {
  it("stale version: shows the conflict, reloads the latest version, never merges", async () => {
    let version = 1;
    const { calls } = mockFetch({
      "GET /api/v1/artists/a1": () => json(200, artist({ version, name: version === 1 ? "Old name" : "Their name" })),
      "PATCH /api/v1/artists/a1": () => {
        version = 2;
        return problem(409, "version_conflict", { current_version: 2 });
      },
    });
    renderIntl(<ArtistEdit id="a1" />);
    const name = await screen.findByLabelText("Name");
    expect(name).toHaveValue("Old name");
    await userEvent.clear(name);
    await userEvent.type(name, "My name");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Someone else changed this")).toBeInTheDocument();
    expect((await body(calls.at(-1)!)).expected_version).toBe(1);

    await userEvent.click(screen.getByRole("button", { name: "Load latest version" }));
    expect(await screen.findByDisplayValue("Their name")).toBeInTheDocument();
    expect(screen.queryByText("Someone else changed this")).not.toBeInTheDocument();
    expect(push).not.toHaveBeenCalled();
  });

  it("forbidden: another member's record shows a permission error", async () => {
    mockFetch({
      "GET /api/v1/artists/a1": () => json(200, artist()),
      "PATCH /api/v1/artists/a1": () => problem(403, "forbidden"),
    });
    renderIntl(<ArtistEdit id="a1" />);
    await userEvent.click(await screen.findByRole("button", { name: "Save" }));
    expect(await screen.findByText("You do not have permission to do this.")).toBeInTheDocument();
  });
});

describe("artist list", () => {
  it("loads, pages with the cursor, and filters by prefix", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/artists": () => {
        const q = new URL(calls.at(-1)!.url).searchParams;
        if (q.get("prefix") === "zz") return json(200, { items: [], next_cursor: null });
        return q.get("cursor")
          ? json(200, { items: [artist({ id: "a2", name: "Second" })], next_cursor: null })
          : json(200, { items: [artist({ name: "First" })], next_cursor: "cur1" });
      },
    });
    renderIntl(<ArtistList query={{}} />);
    expect(screen.getByRole("status")).toHaveTextContent("Loading…");
    expect(await screen.findByRole("link", { name: "First" })).toHaveAttribute("href", "/artists/a1");
    await userEvent.click(screen.getByRole("button", { name: "Load more" }));
    expect(await screen.findByRole("link", { name: "Second" })).toBeInTheDocument();
    expect(new URL(calls.at(-1)!.url).searchParams.get("cursor")).toBe("cur1");
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();

    await userEvent.type(screen.getByLabelText("Starts with"), "zz");
    await userEvent.click(screen.getByRole("button", { name: "Filter" }));
    expect(await screen.findByText("No artists found.")).toBeInTheDocument();
  });

  it("server failure shows an error with retry", async () => {
    let n = 0;
    mockFetch({
      "GET /api/v1/artists": () => (n++ === 0 ? problem(503, "unavailable") : json(200, { items: [artist()], next_cursor: null })),
    });
    renderIntl(<ArtistList query={{}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("link", { name: "فيروز" })).toBeInTheDocument();
  });
});

describe("artist detail", () => {
  it("renders in Arabic with names verbatim and credits linked", async () => {
    mockFetch({
      "GET /api/v1/artists/a1": () =>
        json(200, artist({ credits: [{
          target_type: "composition", target_id: "c1", title: "Kaatrin Mozhi", title_display: disp("Kaatrin Mozhi"),
          role: "composer", role_id: "role1", role_category: "original_songwriter",
          role_display: disp("ملحّن", { language: "ar", dir: "rtl" }),
        }] })),
    });
    renderIntl(<ArtistDetail id="a1" />, "ar");
    const heading = await screen.findByRole("heading", { level: 1 });
    expect(within(heading).getByText("فيروز")).toHaveAttribute("translate", "no");
    expect(screen.getByText("شخص")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Kaatrin Mozhi" })).toHaveAttribute("href", "/compositions/c1");
    expect(screen.getByText("ملحّن", { exact: false })).toBeInTheDocument();
  });

  it("not found shows the localized error", async () => {
    mockFetch({ "GET /api/v1/artists/a1": () => problem(404, "not_found") });
    renderIntl(<ArtistDetail id="a1" />);
    expect(await screen.findByText("We could not find what you were looking for.")).toBeInTheDocument();
  });
});
