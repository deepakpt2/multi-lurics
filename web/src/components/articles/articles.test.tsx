import { describe, expect, it } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, renderIntl } from "@/test/utils";
import { ArticleBody } from "./markdown";
import { ArticleEditor } from "./article-editor";
import { ArticleView } from "./article-view";
import { HomeArticles } from "./home-articles";

const me = (roles: string[]) => ({ id: "u1", email: "u@example.com", display_name: "U", roles, email_verified: true });
const article = (over: Record<string, unknown> = {}) => ({
  id: "a1",
  slug: "onam-2026",
  title: "Onam 2026",
  summary: "Celebrate",
  body_md: "## Hello\n\nText",
  cover_image_id: null,
  status: "published",
  published_at: "2026-08-20T00:00:00Z",
  on_home: false,
  home_from: null,
  home_until: null,
  home_now: false,
  updated_at: "2026-08-20T00:00:00Z",
  version: 3,
  ...over,
});

describe("ArticleBody", () => {
  it("shows Markdown in the site's style and never inserts raw HTML, scripts or unsafe links and pictures", () => {
    const { container } = renderIntl(
      <ArticleBody markdown={'# Big\n\n<script>alert(1)</script><b>raw</b>\n\n[bad](javascript:alert(1)) [ok](https://example.com)\n\n![x](http://evil/a.png) ![y](/api/v1/media/abc/960)'} />,
    );
    expect(screen.getByRole("heading", { level: 2, name: "Big" })).toBeInTheDocument();
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("b")).toBeNull();
    expect(container.querySelector('a[href^="javascript"]')).toBeNull();
    expect(screen.getByRole("link", { name: "ok" })).toHaveAttribute("target", "_blank");
    expect([...container.querySelectorAll("img")].map((i) => i.getAttribute("src"))).toEqual(["/api/v1/media/abc/960"]);
  });
});

describe("HomeArticles", () => {
  it("takes no space when nothing is in its period", async () => {
    mockFetch({ "GET /api/v1/articles/home": () => json(200, { items: [] }) });
    const { container } = renderIntl(<HomeArticles />);
    await waitFor(() => expect(container).toBeEmptyDOMElement());
  });
  it("shows the article with a link to read it", async () => {
    mockFetch({ "GET /api/v1/articles/home": () => json(200, { items: [article({ home_now: true })] }) });
    renderIntl(<HomeArticles />);
    expect(await screen.findByRole("heading", { name: "Onam 2026" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Read/ })).toHaveAttribute("href", "/articles/onam-2026");
  });
});

describe("ArticleView", () => {
  it("readers see the article and no controls", async () => {
    mockFetch({ "GET /api/v1/me": () => json(401, { code: "unauthenticated", status: 401, title: "x", type: "x" }), "GET /api/v1/articles/onam-2026": () => json(200, article()) });
    renderIntl(<ArticleView slug="onam-2026" />);
    expect(await screen.findByRole("heading", { level: 1, name: "Onam 2026" })).toBeInTheDocument();
    expect(screen.queryByText("Manage this article")).not.toBeInTheDocument();
  });

  it("a moderator switches it onto the home page for a period from the article page", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/me": () => json(200, me(["moderator"])),
      "GET /api/v1/articles/onam-2026": () => json(200, article()),
      "PUT /api/v1/articles/a1/home": () => json(200, article({ on_home: true, home_now: true, home_until: "2026-09-01T00:00:00Z" })),
    });
    renderIntl(<ArticleView slug="onam-2026" />);
    await userEvent.type(await screen.findByLabelText(/Hide after/), "2030-09-01T10:00");
    await userEvent.click(screen.getByRole("button", { name: "Show on the home page" }));
    expect(await screen.findByText(/Home page: showing now/)).toBeInTheDocument();
    const body = (await calls.find((c) => c.method === "PUT")!.json()) as Record<string, unknown>;
    expect(body.on_home).toBe(true);
    expect(body.home_from).toBeNull();
    expect(typeof body.home_until).toBe("string");
    expect(screen.getByRole("button", { name: "Remove from the home page" })).toBeInTheDocument();
  });

  it("a draft cannot be put on the home page until it is published", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me(["moderator"])), "GET /api/v1/articles/onam-2026": () => json(200, article({ status: "draft", published_at: null })) });
    renderIntl(<ArticleView slug="onam-2026" />);
    expect(await screen.findByText("Publish the article to show it on the home page.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Show on the home page" })).not.toBeInTheDocument();
  });
});

describe("ArticleEditor", () => {
  it("is for moderators only", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me([])) });
    renderIntl(<ArticleEditor />);
    expect(await screen.findByText("Only moderators can write articles.")).toBeInTheDocument();
  });

  it("fills the fields from a Markdown file and saves a draft", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/me": () => json(200, me(["moderator"])),
      "POST /api/v1/articles": () => json(201, article({ status: "draft" })),
    });
    renderIntl(<ArticleEditor />);
    const file = new File(["---\ntitle: Vishu\nsummary: Spring\n---\n\nHappy **Vishu**"], "vishu.md", { type: "text/markdown" });
    await userEvent.upload(await screen.findByLabelText("Load a .md file", { selector: "input" }), file);
    await waitFor(() => expect(screen.getByLabelText("Title")).toHaveValue("Vishu"));
    expect(screen.getByLabelText("Short summary")).toHaveValue("Spring");
    expect(screen.getByLabelText("Text (Markdown)")).toHaveValue("Happy **Vishu**");
    expect(screen.getByText("Save the article first, then add pictures here.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Save draft" }));
    await waitFor(() => expect(calls.some((c) => c.method === "POST" && c.url.endsWith("/api/v1/articles"))).toBe(true));
    const sent = (await calls.find((c) => c.method === "POST" && c.url.endsWith("/api/v1/articles"))!.json()) as Record<string, unknown>;
    expect(sent).toMatchObject({ title: "Vishu", summary: "Spring", slug: null, body_md: "Happy **Vishu**", publish: false });
  });
});
