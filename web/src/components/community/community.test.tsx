import { describe, expect, it } from "vitest";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { CommunitySection, MyComments, ProfileView } from "./community";
import { AppealQueue, ReportQueue } from "./moderation";

const T = "2026-01-01T00:00:00Z";
const engagement = (over: Record<string, unknown> = {}) => ({
  rating_count: 2, rating_average: 4.5, comment_count: 1, reactions: [{ kind: "like", count: 3 }],
  mine: { rating: null, reactions: [], bookmarked: false }, ...over,
});
const comment = (id: string, body: string, over: Record<string, unknown> = {}) => ({
  id, contribution_id: "c1", author_id: "u2", author_name: "Reader", body, status: "visible", version: 1, created_at: T, edited_at: null, ...over,
});
const base = {
  "GET /api/v1/contributions/c1/engagement": () => json(200, engagement()),
  "GET /api/v1/contributions/c1/comments": () => json(200, { items: [comment("m1", "ما شاء الله")], next_cursor: null }),
};

describe("community section", () => {
  it("shows the average, counts and comments, with each comment in its own direction", async () => {
    mockFetch(base);
    renderIntl(<CommunitySection contributionId="c1" />);
    expect(await screen.findByTestId("rating-summary")).toHaveTextContent("4.5 average from 2 ratings");
    expect(screen.getByRole("button", { name: /Like/ })).toHaveTextContent("3");
    expect(await screen.findByText("ما شاء الله")).toHaveAttribute("dir", "auto");
  });

  it("explains a refused self-rating instead of a generic failure", async () => {
    mockFetch({
      ...base,
      "PUT /api/v1/contributions/c1/rating": () => problem(422, "validation_failed", { errors: [{ field: "contribution_id", code: "self_rating" }] }),
    });
    renderIntl(<CommunitySection contributionId="c1" />);
    await userEvent.click(await screen.findByRole("button", { name: "Rate 5 out of 5" }));
    expect(await screen.findByText("You cannot rate your own work.")).toBeInTheDocument();
  });

  it("toggles a reaction and a bookmark through the API and reloads the totals", async () => {
    let on = false;
    const { calls } = mockFetch({
      ...base,
      "GET /api/v1/contributions/c1/engagement": () => json(200, engagement({ mine: { rating: null, reactions: on ? ["like"] : [], bookmarked: false } })),
      "PUT /api/v1/contributions/c1/reactions/like": () => {
        on = true;
        return new Response(null, { status: 204 });
      },
    });
    renderIntl(<CommunitySection contributionId="c1" />);
    const like = await screen.findByRole("button", { name: /Like/ });
    expect(like).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(like);
    expect(await screen.findByRole("button", { name: /Like/ })).toHaveAttribute("aria-pressed", "true");
    expect(calls.some((c) => c.method === "PUT" && c.url.endsWith("/reactions/like"))).toBe(true);
  });

  it("shows spam and rate-limit refusals in the member's language, keeping the draft", async () => {
    let n = 0;
    mockFetch({
      ...base,
      "POST /api/v1/contributions/c1/comments": () =>
        n++ === 0
          ? problem(422, "validation_failed", { errors: [{ field: "body", code: "too_many_links" }] })
          : problem(429, "rate_limited", { retry_after_secs: 30 }),
    });
    renderIntl(<CommunitySection contributionId="c1" />);
    const box = await screen.findByLabelText("Add a comment");
    await userEvent.type(box, "http://a http://b http://c");
    await userEvent.click(screen.getByRole("button", { name: "Post" }));
    expect(await screen.findByText("This looks like spam: too many links.")).toBeInTheDocument();
    expect(box).toHaveValue("http://a http://b http://c");
    await userEvent.click(screen.getByRole("button", { name: "Post" }));
    expect(await screen.findByText(/Try again in 30 seconds/)).toBeInTheDocument();
  });

  it("reports a comment with a reason and confirms", async () => {
    const { calls } = mockFetch({ ...base, "POST /api/v1/reports": () => json(201, { id: "r1" }) });
    renderIntl(<CommunitySection contributionId="c1" />);
    const item = (await screen.findByText("ما شاء الله")).closest("li")!;
    await userEvent.click(within(item).getByRole("button", { name: "Report" }));
    await userEvent.selectOptions(within(item).getByLabelText("Reason"), "abuse");
    await userEvent.click(within(item).getByRole("button", { name: "Send report" }));
    expect(await screen.findByText(/A moderator will review this/)).toBeInTheDocument();
    const sent = JSON.parse(await calls.at(-1)!.clone().text());
    expect(sent).toMatchObject({ target_type: "comment", target_id: "m1", reason: "abuse" });
  });

  it("an unpublished or missing contribution shows an error, not content", async () => {
    mockFetch({ "GET /api/v1/contributions/c1/engagement": () => problem(404, "not_found"), "GET /api/v1/contributions/c1/comments": () => problem(404, "not_found") });
    renderIntl(<CommunitySection contributionId="c1" />);
    expect(await screen.findByRole("alert")).toHaveTextContent("We could not find what you were looking for.");
  });
});

describe("profile", () => {
  it("keeps the reputation level and the roles apart", async () => {
    mockFetch({
      "GET /api/v1/users/u1/profile": () =>
        json(200, { user_id: "u1", display_name: "ليلى", bio: "bio", joined_at: T, published_contributions: 3, comments: 1, reputation: 140, level: "trusted", roles: [] }),
    });
    renderIntl(<ProfileView userId="u1" />);
    expect(await screen.findByTestId("level")).toHaveTextContent("Trusted");
    expect(screen.queryByTestId("role")).not.toBeInTheDocument();
    expect(screen.getByText("140")).toBeInTheDocument();
  });

  it("shows a moderator badge only when the role is held", async () => {
    mockFetch({
      "GET /api/v1/users/u1/profile": () =>
        json(200, { user_id: "u1", display_name: "M", bio: "", joined_at: T, published_contributions: 0, comments: 0, reputation: 0, level: "newcomer", roles: ["moderator"] }),
    });
    renderIntl(<ProfileView userId="u1" />, "ar");
    expect(await screen.findByTestId("role")).toHaveTextContent("مشرف");
    expect(screen.getByTestId("level")).toHaveTextContent("عضو جديد");
  });
});

describe("my comments and appeals", () => {
  it("lets the author appeal a hidden comment once, and shows server refusals", async () => {
    let n = 0;
    mockFetch({
      "GET /api/v1/me/comments": () => json(200, { items: [comment("m1", "visible one"), comment("m2", "hidden one", { status: "hidden" })], next_cursor: null }),
      "POST /api/v1/appeals": () => (n++ === 0 ? problem(409, "appeal_pending") : json(201, { id: "a1" })),
    });
    renderIntl(<MyComments />);
    const hidden = (await screen.findByText("hidden one")).closest("li")!;
    expect(within(screen.getByText("visible one").closest("li")!).queryByRole("button", { name: "Appeal" })).not.toBeInTheDocument();
    await userEvent.click(within(hidden).getByRole("button", { name: "Appeal" }));
    await userEvent.type(within(hidden).getByLabelText("Why should it be restored?"), "it was fine");
    await userEvent.click(within(hidden).getByRole("button", { name: "Appeal" }));
    expect(await within(hidden).findByText("An appeal is already open.")).toBeInTheDocument();
    await userEvent.click(within(hidden).getByRole("button", { name: "Appeal" }));
    expect(await within(hidden).findByText("Appeal sent.")).toBeInTheDocument();
  });
});

describe("moderation queues", () => {
  const report = (id: string, type: string) => ({ id, reporter_id: "u3", target_type: type, target_id: "t1", reason: "abuse", detail: "rude", status: "open", resolved_by: null, resolution_note: null, resolved_at: null, created_at: T });

  it("resolves a comment report, optionally hiding the comment, and refreshes the queue", async () => {
    let open = true;
    const { calls } = mockFetch({
      "GET /api/v1/moderation/reports": () => json(200, { items: open ? [report("r1", "comment")] : [], next_cursor: null }),
      "POST /api/v1/moderation/reports/r1/resolve": () => {
        open = false;
        return json(200, report("r1", "comment"));
      },
    });
    renderIntl(<ReportQueue />);
    await userEvent.type(await screen.findByLabelText("Note"), "clear abuse");
    await userEvent.click(screen.getByRole("button", { name: "Uphold and hide comment" }));
    expect(await screen.findByText("No open reports.")).toBeInTheDocument();
    expect(JSON.parse(await calls.find((c) => c.method === "POST")!.clone().text())).toEqual({ resolution: "upheld", action: "hide_comment", note: "clear abuse" });
  });

  it("offers hiding only for comments, and shows a conflict or refusal without losing the item", async () => {
    mockFetch({
      "GET /api/v1/moderation/reports": () => json(200, { items: [report("r2", "contribution")], next_cursor: null }),
      "POST /api/v1/moderation/reports/r2/resolve": () => problem(409, "already_resolved"),
    });
    renderIntl(<ReportQueue />);
    expect(await screen.findByRole("button", { name: "Uphold" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Uphold and hide comment" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByTestId("report")).toBeInTheDocument();
  });

  it("a member opening the queue sees the permission error, not an empty queue", async () => {
    mockFetch({ "GET /api/v1/moderation/reports": () => problem(403, "forbidden") });
    renderIntl(<ReportQueue />);
    expect(await screen.findByText("You do not have permission to do this.")).toBeInTheDocument();
  });

  it("appeals can be restored or denied; the same moderator gets the explanation", async () => {
    mockFetch({
      "GET /api/v1/moderation/appeals": () => json(200, { items: [{ id: "a1", appellant_id: "u2", target_type: "comment", target_id: "m1", message: "it was fine", status: "open", decided_by: null, decision_note: null, decided_at: null, created_at: T }], next_cursor: null }),
      "POST /api/v1/moderation/appeals/a1/decide": () => problem(403, "own_decision"),
    });
    renderIntl(<AppealQueue />);
    await userEvent.click(await screen.findByRole("button", { name: "Restore comment" }));
    expect(await screen.findByText("A different moderator must review this.")).toBeInTheDocument();
  });
});
