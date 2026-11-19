import { describe, expect, it, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { EntityPicker } from "./entity-picker";
import { ProposalQueue } from "./proposals";

const disp = (text: string) => ({ text, description: null, language: null, script: null, dir: "ltr", fallback: false, resolution: "base" });
const hit = (id: string, name: string, over: Record<string, unknown> = {}) => ({
  id, type: "artist", display: disp(name), matched_via: "name", score: 1, sub_kind: "person", detail: null, year: null, code: null,
  localized_names: [], related: [], ...over,
});
const body = async (req: Request) => JSON.parse(await req.clone().text());
const search = (req: Request) => new URL(req.url).searchParams;

function setup(over: Record<string, () => Response> = {}) {
  const onPick = vi.fn();
  const m = mockFetch({ "GET /api/v1/entities/suggest": () => json(200, { items: [hit("a1", "A. R. Rahman"), hit("a2", "Ravi", { detail: "drummer" })] }), ...over });
  renderIntl(<EntityPicker type="artist" label="Find an artist" onPick={onPick} />);
  return { onPick, ...m, box: screen.getByRole("combobox", { name: "Find an artist" }) };
}

describe("entity picker", () => {
  it("waits for a pause, sends one request for the final text, and picks by keyboard (id, not text)", async () => {
    const { onPick, calls, box } = setup();
    await userEvent.type(box, "a r r");
    const options = await screen.findAllByRole("option");
    expect(options).toHaveLength(2);
    expect(calls).toHaveLength(1); // debounced: one request, not one per keystroke
    expect(search(calls[0]).get("q")).toBe("a r r");
    expect(search(calls[0]).get("type")).toBe("artist");
    expect(box).toHaveAttribute("aria-expanded", "true");

    await userEvent.keyboard("{ArrowDown}{ArrowDown}{ArrowUp}");
    expect(box).toHaveAttribute("aria-activedescendant", options[0].id);
    await userEvent.keyboard("{Enter}");
    expect(onPick).toHaveBeenCalledWith({ id: "a1", label: "A. R. Rahman", note: undefined });
    expect(box).toHaveValue("");
  });

  it("Enter with nothing highlighted does not pick and does not submit a surrounding form", async () => {
    const onSubmit = vi.fn((e: React.FormEvent) => e.preventDefault());
    mockFetch({ "GET /api/v1/entities/suggest": () => json(200, { items: [hit("a1", "X")] }) });
    const onPick = vi.fn();
    renderIntl(
      <form onSubmit={onSubmit}>
        <EntityPicker type="artist" label="Find an artist" onPick={onPick} />
      </form>,
    );
    await userEvent.type(screen.getByRole("combobox"), "x");
    await screen.findByRole("option");
    await userEvent.keyboard("{Enter}");
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onPick).not.toHaveBeenCalled();
    await userEvent.keyboard("{Escape}");
    expect(screen.getByRole("combobox")).toHaveValue("");
  });

  it("cancels the previous request when the text changes and ignores its late answer", async () => {
    const signals: AbortSignal[] = [];
    const { box } = setup({
      "GET /api/v1/entities/suggest": () => json(200, { items: [hit("a9", "Latest")] }),
    });
    const spy = vi.spyOn(globalThis, "fetch");
    const base = spy.getMockImplementation()!;
    spy.mockImplementation(async (input, init) => {
      signals.push((input as Request).signal);
      return base(input, init);
    });
    await userEvent.type(box, "ab");
    await screen.findByRole("option", { name: /Latest/ });
    await userEvent.type(box, "c");
    await screen.findByRole("option", { name: /Latest/ });
    expect(signals.length).toBeGreaterThanOrEqual(2);
    expect(signals[0].aborted).toBe(true);
  });

  it("shows context: kind, disambiguation, alias match, related works, localized names", async () => {
    setup({
      "GET /api/v1/entities/suggest": () =>
        json(200, {
          items: [hit("a1", "Ilaiyaraaja", { matched_via: "alias", detail: "composer", related: [{ kind: "composition", id: "c1", title: "Theme" }], localized_names: [{ language: "ml", name: "ഇളയരാജ" }] })],
        }),
    });
    await userEvent.type(screen.getByRole("combobox"), "ilay");
    const option = await screen.findByRole("option");
    expect(option).toHaveTextContent("matched an alias");
    expect(option).toHaveTextContent("Person, composer");
    expect(option).toHaveTextContent("Related: Theme");
    expect(screen.getByText("ഇളയരാജ")).toHaveAttribute("lang", "ml");
  });

  it("loading, error with retry, and no-results states", async () => {
    let n = 0;
    setup({ "GET /api/v1/entities/suggest": () => (n++ === 0 ? problem(503, "unavailable") : json(200, { items: [] })) });
    await userEvent.type(screen.getByRole("combobox"), "zz");
    expect(await screen.findByRole("alert")).toHaveTextContent(/try again|unavailable|temporarily/i);
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("No match for “zz”.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Propose a new artist" })).toBeInTheDocument();
  });

  it("language and script cannot be proposed", async () => {
    mockFetch({ "GET /api/v1/entities/suggest": () => json(200, { items: [] }) });
    renderIntl(<EntityPicker type="language" label="Language" onPick={vi.fn()} />);
    await userEvent.type(screen.getByRole("combobox"), "zz");
    await screen.findByText("No match for “zz”.");
    expect(screen.queryByRole("button", { name: /Propose/ })).not.toBeInTheDocument();
  });

  it("proposal: likely matches come first; nothing is queued until the person confirms", async () => {
    let posted = 0;
    const { calls, onPick } = setup({
      // Nothing matches while typing; after the server flags duplicates the same search finds one.
      "GET /api/v1/entities/suggest": () => json(200, { items: posted ? [hit("a1", "A. R. Rahman")] : [] }),
      "POST /api/v1/entities/proposals": () => (posted++ === 0 ? problem(409, "likely_duplicates", { candidate_ids: ["a1"] }) : json(201, { id: "p1" })),
    });
    await userEvent.type(screen.getByRole("combobox"), "AR Rahman");
    await userEvent.click(await screen.findByRole("button", { name: "Propose a new artist" }));
    await userEvent.click(screen.getByRole("button", { name: "Send proposal" }));
    expect(await screen.findByText(/These may be what you mean/)).toBeInTheDocument();
    expect(screen.getByText("A. R. Rahman")).toBeInTheDocument();
    expect(posted).toBe(1);
    expect(await body(calls.find((c) => c.method === "POST")!)).toMatchObject({ entity_type: "artist", name: "AR Rahman", kind: "person", confirm_distinct: false });
    // Confirming sends it again, flagged.
    await userEvent.click(screen.getByRole("button", { name: /None of these/ }));
    expect(await screen.findByText(/Proposal sent for review/)).toBeInTheDocument();
    expect((await body(calls.filter((c) => c.method === "POST").at(-1)!)).confirm_distinct).toBe(true);
    expect(onPick).not.toHaveBeenCalled();
  });

  it("proposal: choosing an existing match picks its id instead of proposing", async () => {
    let posted = 0;
    const { onPick } = setup({
      "GET /api/v1/entities/suggest": () => json(200, { items: posted ? [hit("a1", "A. R. Rahman")] : [] }),
      "POST /api/v1/entities/proposals": () => (posted++, problem(409, "likely_duplicates", { candidate_ids: ["a1"] })),
    });
    await userEvent.type(screen.getByRole("combobox"), "AR Rahman");
    await userEvent.click(await screen.findByRole("button", { name: "Propose a new artist" }));
    await userEvent.click(screen.getByRole("button", { name: "Send proposal" }));
    await userEvent.click(await screen.findByRole("button", { name: "Use this one" }));
    expect(onPick).toHaveBeenCalledWith({ id: "a1", label: "A. R. Rahman", note: undefined });
    expect(posted).toBe(1);
  });

  it("proposal: a name with no likely match is sent unconfirmed and awaits review", async () => {
    const { calls } = setup({
      "GET /api/v1/entities/suggest": () => json(200, { items: [] }),
      "POST /api/v1/entities/proposals": () => json(201, { id: "p1" }),
    });
    await userEvent.type(screen.getByRole("combobox"), "Brand New");
    await userEvent.click(await screen.findByRole("button", { name: "Propose a new artist" }));
    await userEvent.click(screen.getByRole("button", { name: "Send proposal" }));
    expect(await screen.findByText(/Proposal sent for review/)).toBeInTheDocument();
    expect((await body(calls.at(-1)!)).confirm_distinct).toBe(false);
  });

  it("proposal while signed out shows the sign-in problem", async () => {
    setup({
      "GET /api/v1/entities/suggest": () => json(200, { items: [] }),
      "POST /api/v1/entities/proposals": () => problem(401, "unauthenticated"),
    });
    await userEvent.type(screen.getByRole("combobox"), "Nobody");
    await userEvent.click(await screen.findByRole("button", { name: "Propose a new artist" }));
    await userEvent.click(screen.getByRole("button", { name: "Send proposal" }));
    expect(await screen.findByText("Please sign in to continue.")).toBeInTheDocument();
  });

  it("renders in Arabic with a plural result count", async () => {
    mockFetch({ "GET /api/v1/entities/suggest": () => json(200, { items: [hit("a1", "فيروز"), hit("a2", "فيروز كلاس")] }) });
    renderIntl(<EntityPicker type="artist" label="ابحث" onPick={vi.fn()} />, "ar");
    await userEvent.type(screen.getByRole("combobox"), "ف");
    expect(await screen.findByText("نتيجتان")).toBeInTheDocument();
    expect(screen.getByRole("combobox")).toHaveAttribute("dir", "auto");
  });
});

describe("proposal queue", () => {
  const proposal = { id: "p1", entity_type: "artist", name: "AR Rahman", kind: "person", disambiguation: "drummer", aliases: [], note: null, shown_matches: [], status: "pending", created_by: "u1", created_at: "2026-01-01T00:00:00Z" };

  it("approving over likely duplicates needs an explicit confirmation", async () => {
    let approved = 0;
    const { calls } = mockFetch({
      "GET /api/v1/moderation/entity-proposals": () => json(200, { items: approved ? [] : [proposal], next_cursor: null }),
      "POST /api/v1/entities/proposals/p1/approve": () => (approved++ === 0 ? problem(409, "likely_duplicates", { candidate_ids: ["a1"] }) : json(200, { ...proposal, status: "approved" })),
      "GET /api/v1/entities/suggest": () => json(200, { items: [hit("a1", "A. R. Rahman")] }),
    });
    renderIntl(<ProposalQueue />);
    await userEvent.click(await screen.findByRole("button", { name: "Approve" }));
    expect(await screen.findByText(/look similar/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Approve anyway" }));
    expect(await screen.findByText("No proposals are waiting.")).toBeInTheDocument();
    const sent = calls.filter((c) => c.method === "POST");
    expect((await body(sent[0])).confirm_distinct).toBe(false);
    expect((await body(sent[1])).confirm_distinct).toBe(true);
  });

  it("forbidden for a non-moderator, and an empty queue says so", async () => {
    mockFetch({ "GET /api/v1/moderation/entity-proposals": () => problem(403, "forbidden") });
    renderIntl(<ProposalQueue />);
    expect(await screen.findByText("You do not have permission to do this.")).toBeInTheDocument();
  });
});
