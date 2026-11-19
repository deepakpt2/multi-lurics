import { describe, expect, it } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { push } from "@/test/setup";
import { ContributionDetail, ContributionEdit, ContributionHistory, ContributionNew, LyricsQueue, LyricsSection } from "./lyrics";

const T = "2026-01-01T00:00:00Z";
const disp = (text: string) => ({ text, description: null, language: "en", script: null, dir: "ltr", fallback: false, resolution: "requested" });
const languages = {
  items: [
    { id: "l1", iso639_3: "mal", iso639_1: "ml", name_en: "Malayalam", endonym: "മലയാളം", is_rtl: false, default_script: "Mlym", is_extension: false },
    { id: "l2", iso639_3: "ara", iso639_1: "ar", name_en: "Arabic", endonym: "العربية", is_rtl: true, default_script: "Arab", is_extension: false },
  ],
  next_cursor: null,
};
const scripts = {
  items: [
    { id: "s0", iso15924: "Mlym", numeric_code: 347, name_en: "Malayalam", is_rtl: false, is_extension: false },
    { id: "s1", iso15924: "Arab", numeric_code: 160, name_en: "Arabic", is_rtl: true, is_extension: false },
  ],
  next_cursor: null,
};
const rev = (id: string, no: number, body: string, over: Record<string, unknown> = {}) => ({
  id, contribution_id: "c1", revision_no: no, parent_revision_id: no > 1 ? `r${no - 1}` : null, body, note: null,
  restored_from_revision_id: null, created_by: "u1", created_at: T, published: false, ...over,
});
const contribution = (over: Record<string, unknown> = {}) => ({
  id: "c1", recording_id: "rec1", type: "translation", language: "ar", script: "Arab", dir: "rtl", is_primary: true,
  status: "published", source_contribution_id: null, created_by: "u1", created_at: T, updated_at: T,
  published: rev("r1", 1, "كلمات\nالأغنية", { published: true }), head: null, current_revision_id: null,
  has_unpublished_changes: false, review_note: null, ...over,
});
const recording = { id: "rec1", title: "Take", display: disp("Take") };
const body = async (req: Request) => JSON.parse(await req.clone().text());

describe("lyric editor: new contribution", () => {
  const routes = { "GET /api/v1/languages": () => json(200, languages), "GET /api/v1/scripts": () => json(200, scripts) };

  it("types in the script's direction, sends a retry key and goes to the new contribution", async () => {
    const { calls } = mockFetch({ ...routes, "POST /api/v1/recordings/rec1/contributions": () => json(201, contribution({ id: "c9" })) });
    renderIntl(<ContributionNew recordingId="rec1" query={{}} />);
    const bodyField = await screen.findByLabelText("Lyrics");
    expect(bodyField).toHaveAttribute("dir", "ltr");
    await userEvent.selectOptions(screen.getByLabelText("Language"), "ara");
    // Language only suggests a usual script; both stay independent fields.
    expect(screen.getByLabelText("Script")).toHaveValue("Arab");
    expect(bodyField).toHaveAttribute("dir", "rtl");
    expect(bodyField).toHaveAttribute("lang", "ar");
    await userEvent.type(bodyField, "سطر أول");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(push).toHaveBeenCalledWith("/contributions/c9"));
    const req = calls.at(-1)!;
    expect(req.headers.get("idempotency-key")).toMatch(/.{8,}/);
    expect(await body(req)).toMatchObject({ type: "original", language: "ara", script: "Arab", body: "سطر أول", note: null });
  });

  it("shows server validation next to the field and does not navigate; unauthenticated asks to sign in", async () => {
    let n = 0;
    mockFetch({
      ...routes,
      "POST /api/v1/recordings/rec1/contributions": () =>
        n++ === 0 ? problem(422, "validation_failed", { errors: [{ field: "body", code: "too_long" }] }) : problem(401, "unauthenticated"),
    });
    renderIntl(<ContributionNew recordingId="rec1" query={{ language: "mal", script: "Mlym" }} />);
    await userEvent.type(await screen.findByLabelText("Lyrics"), "x");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("This is too long.")).toBeInTheDocument();
    expect(screen.getByLabelText("Lyrics")).toHaveAttribute("aria-invalid", "true");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Please sign in to continue.")).toBeInTheDocument();
    expect(push).not.toHaveBeenCalled();
  });
});

describe("contribution detail", () => {
  it("public reader: published text in its own language and direction, no editing or review controls", async () => {
    mockFetch({
      "GET /api/v1/contributions/c1": () => json(200, contribution()),
      "GET /api/v1/recordings/rec1": () => json(200, recording),
      "GET /api/v1/me": () => problem(401, "unauthenticated"),
    });
    renderIntl(<ContributionDetail id="c1" />);
    const text = await screen.findByText((_, el) => el?.tagName === "PRE" && el.textContent === "كلمات\nالأغنية");
    expect(text).toHaveAttribute("lang", "ar");
    expect(text).toHaveAttribute("dir", "rtl");
    expect(screen.getByText("Primary version")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Edit" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Review" })).not.toBeInTheDocument();
  });

  it("author sees unpublished changes separately from what readers see", async () => {
    mockFetch({
      "GET /api/v1/contributions/c1": () =>
        json(200, contribution({ status: "pending", current_revision_id: "r2", has_unpublished_changes: true, head: rev("r2", 2, "draft text") })),
      "GET /api/v1/recordings/rec1": () => json(200, recording),
      "GET /api/v1/me": () => json(200, { id: "u1", roles: [] }),
    });
    renderIntl(<ContributionDetail id="c1" />);
    const section = (await screen.findByText("Latest revision (not public)")).closest("section")!;
    expect(within(section).getByText("draft text")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Edit" })).toHaveAttribute("href", "/contributions/c1/edit");
    expect(screen.queryByRole("button", { name: "Publish" })).not.toBeInTheDocument();
  });

  it("moderator publishes the reviewed revision; a stale decision shows the conflict instead", async () => {
    let stale = true;
    const { calls } = mockFetch({
      "GET /api/v1/contributions/c1": () =>
        json(200, contribution({ status: "pending", published: null, current_revision_id: "r1", head: rev("r1", 1, "x"), has_unpublished_changes: true })),
      "GET /api/v1/recordings/rec1": () => json(200, recording),
      "GET /api/v1/me": () => json(200, { id: "m", roles: ["moderator"] }),
      "POST /api/v1/contributions/c1/publish": () => {
        if (stale) {
          stale = false;
          return problem(409, "stale_revision", { current_revision_id: "r2" });
        }
        return json(200, contribution());
      },
    });
    renderIntl(<ContributionDetail id="c1" />);
    await userEvent.click(await screen.findByRole("button", { name: "Publish" }));
    expect(await screen.findByText("Someone else changed this")).toBeInTheDocument();
    const first = await body(calls.find((c) => c.method === "POST")!);
    expect(first.expected_parent_revision_id).toBe("r1");
    await userEvent.click(screen.getByRole("button", { name: "Publish" }));
    await waitFor(() => expect(calls.filter((c) => c.method === "POST")).toHaveLength(2));
  });

  it("hidden contribution is a plain not-found", async () => {
    mockFetch({ "GET /api/v1/contributions/c1": () => problem(404, "not_found"), "GET /api/v1/me": () => problem(401, "unauthenticated") });
    renderIntl(<ContributionDetail id="c1" />);
    expect(await screen.findByText("We could not find what you were looking for.")).toBeInTheDocument();
  });
});

describe("edit conflict", () => {
  it("stale parent: shows the conflict, keeps the author's text visible, loads the latest on request, never merges", async () => {
    let head = "r1";
    const { calls } = mockFetch({
      "GET /api/v1/contributions/c1": () =>
        json(200, contribution({ current_revision_id: head, head: rev(head, head === "r1" ? 1 : 2, head === "r1" ? "mine v1" : "their v2"), published: null })),
      "POST /api/v1/contributions/c1/revisions": () => {
        head = "r2";
        return problem(409, "stale_revision", { current_revision_id: "r2" });
      },
    });
    renderIntl(<ContributionEdit id="c1" />);
    const field = await screen.findByLabelText("Lyrics");
    expect(field).toHaveValue("mine v1");
    await userEvent.clear(field);
    await userEvent.type(field, "my edit");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Someone else changed this")).toBeInTheDocument();
    expect((await body(calls.at(-1)!)).expected_parent_revision_id).toBe("r1");
    expect(screen.getByText("Your unsaved text")).toBeInTheDocument();
    expect(screen.getByText("my edit")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Load latest version" }));
    await waitFor(() => expect(screen.getByLabelText("Lyrics")).toHaveValue("their v2"));
    expect(screen.queryByText("Someone else changed this")).not.toBeInTheDocument();
    expect(push).not.toHaveBeenCalled();
  });

  it("a member who is not the author is offered an alternative instead of an editor", async () => {
    mockFetch({ "GET /api/v1/contributions/c1": () => json(200, contribution()) });
    renderIntl(<ContributionEdit id="c1" />);
    expect(await screen.findByText(/You can edit only your own contributions/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Add an alternative version" })).toHaveAttribute("href", expect.stringContaining("/recordings/rec1/lyrics/new?type=translation&language=ar"));
    expect(screen.queryByLabelText("Lyrics")).not.toBeInTheDocument();
  });
});

describe("revision history", () => {
  const history = {
    items: [rev("r3", 3, "c", { restored_from_revision_id: "r1" }), rev("r2", 2, "b", { published: true }), rev("r1", 1, "a")].map((r) => ({ ...r, size: 1 })),
    next_cursor: null,
  };

  it("shows changes between revisions and restores an old one as a new revision", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/contributions/c1": () => json(200, contribution({ current_revision_id: "r3", head: rev("r3", 3, "c") })),
      "GET /api/v1/contributions/c1/revisions": () => json(200, history),
      "GET /api/v1/revisions/r2/diff/r3": () =>
        json(200, { from: history.items[1], to: history.items[0], hunks: [{ op: "equal", lines: ["keep"] }, { op: "delete", lines: ["old"] }, { op: "insert", lines: ["new"] }] }),
      "POST /api/v1/contributions/c1/revisions": () => json(201, contribution()),
    });
    renderIntl(<ContributionHistory id="c1" />);
    expect(await screen.findByText("Revision 3")).toBeInTheDocument();
    expect(screen.getByText("Restored")).toBeInTheDocument();
    expect(screen.getByText("Was published")).toBeInTheDocument();
    await userEvent.click(screen.getAllByRole("button", { name: "Show changes" })[0]);
    const diff = await screen.findByRole("group", { name: "Changes" });
    expect(diff.querySelector("del")).toHaveTextContent("old");
    expect(diff.querySelector("ins")).toHaveTextContent("new");

    const restoreButtons = screen.getAllByRole("button", { name: "Restore this version" });
    expect(restoreButtons).toHaveLength(2); // not offered for the current revision
    await userEvent.click(restoreButtons[1]);
    await waitFor(() => expect(calls.some((c) => c.method === "POST")).toBe(true));
    expect(await body(calls.find((c) => c.method === "POST")!)).toMatchObject({ expected_parent_revision_id: "r3", restore_from_revision_id: "r1" });
    expect(await screen.findByText("Restored as a new revision. Nothing was deleted.")).toBeInTheDocument();
  });

  it("anonymous reader sees the public history without restore buttons", async () => {
    mockFetch({
      "GET /api/v1/contributions/c1": () => json(200, contribution()),
      "GET /api/v1/contributions/c1/revisions": () => json(200, { items: [history.items[1]], next_cursor: null }),
    });
    renderIntl(<ContributionHistory id="c1" />);
    expect(await screen.findByText("Revision 2")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Restore this version" })).not.toBeInTheDocument();
  });
});

describe("lists and queues", () => {
  it("recording page lists published lyrics, with an empty state", async () => {
    mockFetch({ "GET /api/v1/recordings/rec1/contributions": () => json(200, { items: [], next_cursor: null }) });
    renderIntl(<LyricsSection recordingId="rec1" />);
    expect(await screen.findByText("No lyrics have been added yet.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Add lyrics" })).toHaveAttribute("href", "/recordings/rec1/lyrics/new");
  });

  it("moderation queue: loading, items with a review link, empty and forbidden states", async () => {
    mockFetch({ "GET /api/v1/moderation/contributions": () => json(200, { items: [contribution({ status: "pending" })], next_cursor: null }) });
    renderIntl(<LyricsQueue />);
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "Review" })).toHaveAttribute("href", "/contributions/c1");
    expect(screen.getByText("Awaiting review", { exact: false })).toBeInTheDocument();
  });

  it("queue shows empty and forbidden", async () => {
    mockFetch({ "GET /api/v1/moderation/contributions": () => json(200, { items: [], next_cursor: null }) });
    const { unmount } = renderIntl(<LyricsQueue />);
    expect(await screen.findByText("Nothing is waiting for review.")).toBeInTheDocument();
    unmount();
    mockFetch({ "GET /api/v1/moderation/contributions": () => problem(403, "forbidden") });
    renderIntl(<LyricsQueue />);
    expect(await screen.findByText("You do not have permission to do this.")).toBeInTheDocument();
  });
});
