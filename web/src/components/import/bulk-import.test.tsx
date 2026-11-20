import { describe, expect, it } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, renderIntl } from "@/test/utils";
import { BulkImport } from "./bulk-import";

const user = (roles: string[]) => ({ id: "u1", email: "u@example.com", display_name: "U", roles, email_verified: true });
const file = (text: string, name = "songs.csv") => new File([text], name, { type: "text/csv" });
const rows = (n: number) => "title,language,lyrics\n" + Array.from({ length: n }, (_, i) => `Song ${i + 1},ml,വരി ${i + 1}`).join("\n");

const result = (row: number, title: string, status: string, extra = {}) => ({ row, title, status, reason: null, problems: [], composition_id: null, ...extra });

describe("bulk import", () => {
  it("is for moderators only", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, user([])) });
    renderIntl(<BulkImport />);
    expect(await screen.findByText("Only moderators can import songs.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Choose a file" })).not.toBeInTheDocument();
  });

  it("walks through template, file, check and import, in batches, and says what happened to each row", async () => {
    const seen: { dry: boolean; titles: string[] }[] = [];
    mockFetch({
      "GET /api/v1/me": () => json(200, user(["moderator"])),
      "POST /api/v1/import/songs": () => json(200, {}), // replaced below, per request
    });
    // A richer fake: one response per request, from what was sent.
    const spy = (await import("vitest")).vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const req = input as Request;
      const path = new URL(req.url, "http://localhost").pathname;
      if (path === "/api/v1/me") return json(200, user(["moderator"]));
      const body = (await req.json()) as { rows: { title: string }[]; dry_run: boolean };
      seen.push({ dry: body.dry_run, titles: body.rows.map((r) => r.title) });
      const results = body.rows.map((r, i) => (r.title === "Song 2" ? result(i + 1, r.title, "failed", { problems: [{ field: "language", code: "unknown_language" }] }) : r.title === "Song 3" ? result(i + 1, r.title, "skipped", { reason: "already_exists" }) : result(i + 1, r.title, body.dry_run ? "would_create" : "created")));
      return json(200, { dry_run: body.dry_run, results, created: 0, skipped: 0, failed: 0 });
    });
    renderIntl(<BulkImport />);
    await screen.findByRole("button", { name: "Choose a file" });
    expect(screen.getByRole("button", { name: "Download CSV template" })).toBeInTheDocument();

    // Nothing to check or import before a file is chosen.
    expect(screen.getByRole("button", { name: "Check the file" })).toBeDisabled();
    await userEvent.upload(screen.getByLabelText("Choose a file", { selector: "input" }), file(rows(30)));
    expect(await screen.findByText(/30 rows/)).toBeInTheDocument();
    expect(screen.getByText("Columns found: title, language, lyrics")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Import 0 songs/ })).toBeDisabled();

    await userEvent.click(screen.getByRole("button", { name: "Check the file" }));
    const summary = (await screen.findByRole("heading", { name: "Check result" })).closest("section")!;
    // 30 rows go in two requests of at most 25, all as dry runs.
    expect(seen.map((s) => [s.dry, s.titles.length])).toEqual([[true, 25], [true, 5]]);
    expect(within(summary).getByText("28 songs are ready to import")).toBeInTheDocument();
    expect(within(summary).getByText("1 song already in the catalogue (skipped)")).toBeInTheDocument();
    expect(within(summary).getByText("1 row with a problem")).toBeInTheDocument();
    // The row number is the sheet's own (header is row 1), and the problem is in words.
    const bad = within(summary).getByText("Song 2").closest("tr")!;
    expect(within(bad).getByText("3")).toBeInTheDocument();
    expect(within(bad).getByText(/Language of the lyrics.*We do not know that language|Language/)).toBeInTheDocument();
    expect(within(summary).getByText("Song 3").closest("tr")).toHaveTextContent("Already in the catalogue.");

    await userEvent.click(screen.getByRole("button", { name: "Import 28 songs" }));
    await waitFor(() => expect(screen.getByRole("heading", { name: "Import finished" })).toBeInTheDocument());
    expect(seen.slice(2).map((s) => [s.dry, s.titles.length])).toEqual([[false, 25], [false, 5]]);
    expect(screen.getByText("28 songs imported")).toBeInTheDocument();
    spy.mockRestore();
  });

  it("warns about columns it does not use and refuses a file with no title column", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, user(["admin"])) });
    renderIntl(<BulkImport />);
    await screen.findByRole("button", { name: "Choose a file" });
    await userEvent.upload(screen.getByLabelText("Choose a file", { selector: "input" }), file("foo,bar\n1,2"));
    expect(await screen.findByText("There is no title column, so nothing can be imported.")).toBeInTheDocument();
    expect(screen.getByText("These columns are not used: foo, bar")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check the file" })).toBeDisabled();
  });
});
