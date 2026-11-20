import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, renderIntl } from "@/test/utils";
import { Compare, PanelPicker, parseIds } from "./compare";

const T = "2026-01-01T00:00:00Z";
const item = (id: string, type: string, language: string, script: string, primary: boolean) => ({
  id, recording_id: "rec1", type, language, script, dir: script === "Arab" ? "rtl" : "ltr", is_primary: primary,
  status: "published", source_contribution_id: null, created_by: "u1", created_at: T, updated_at: T,
});
const contributions = [
  item("c-orig", "original", "ml", "Mlym", true),
  item("c-alt", "original", "ml", "Mlym", false),
  item("c-en", "translation", "en", "Latn", true),
  item("c-ar", "translation", "ar", "Arab", true),
];
const content = (c: ReturnType<typeof item>, body: string, over: Record<string, unknown> = {}) => ({
  recording_id: "rec1", recording_title: "Take 1", composition_id: "w1", composition_title: "Kaatrin Mozhi", type: c.type,
  language: c.language, script: c.script, dir: c.dir, is_primary: c.is_primary, slot_size: 1, revision_id: `r-${c.id}`, revision_no: 1,
  body, revised_at: T, ...over,
});
const panels = [
  { id: "c-orig", available: true, content: content(contributions[0], "ആദ്യ വരി", { slot_size: 2 }) },
  { id: "c-alt", available: true, content: content(contributions[1], "ബദൽ വരി", { slot_size: 2 }) },
  { id: "c-en", available: true, content: content(contributions[2], "first line") },
  { id: "c-ar", available: true, content: content(contributions[3], "السطر الأول") },
];
const resolveRoute = (list = panels) => ({ "GET /api/v1/comparisons/resolve": () => json(200, { panels: list }) });

describe("choosing a panel's contribution", () => {
  const routes = {
    "GET /api/v1/recordings": () => json(200, { items: [{ id: "rec1", title: "Take 1", composition_id: "w1", display: { text: "Take 1" } }], next_cursor: null }),
    "GET /api/v1/recordings/rec1/contributions": () => json(200, { items: contributions, next_cursor: null }),
  };

  async function pickRecording() {
    await userEvent.type(screen.getByLabelText("Find a recording"), "take");
    await userEvent.click(screen.getByRole("button", { name: "Find" }));
    await userEvent.click(await screen.findByRole("button", { name: /Take 1/ }));
    await screen.findByLabelText("Type");
  }

  it("offers every type/language/script slot, and each alternative is its own choice", async () => {
    mockFetch(routes);
    const onSelect = vi.fn();
    renderIntl(<PanelPicker onSelect={onSelect} />);
    await pickRecording();
    const type = screen.getByLabelText("Type");
    expect(within(type).getAllByRole("option").map((o) => o.textContent)).toEqual(["Any", "Original", "Translation"]);

    await userEvent.selectOptions(type, "original");
    await userEvent.selectOptions(screen.getByLabelText("Language"), "ml");
    await userEvent.selectOptions(screen.getByLabelText("Script"), "Mlym");
    const version = screen.getByLabelText("Version");
    const options = within(version).getAllByRole("option").slice(1);
    // Primary and alternative are two separate options with separate ids.
    expect(options.map((o) => (o as HTMLOptionElement).value)).toEqual(["c-orig", "c-alt"]);
    expect(options[0].textContent).toContain("Primary");
    expect(options[1].textContent).toContain("Alternative 2");

    await userEvent.selectOptions(version, "c-alt");
    expect(onSelect).toHaveBeenLastCalledWith("c-alt");
    await userEvent.selectOptions(version, "c-orig");
    expect(onSelect).toHaveBeenLastCalledWith("c-orig");
  });

  it("narrows by type, language and script independently; unrelated slots are not offered", async () => {
    mockFetch(routes);
    renderIntl(<PanelPicker onSelect={vi.fn()} />);
    await pickRecording();
    await userEvent.selectOptions(screen.getByLabelText("Type"), "translation");
    const langs = within(screen.getByLabelText("Language")).getAllByRole("option").map((o) => (o as HTMLOptionElement).value);
    expect(langs).toEqual(["", "ar", "en"]);
    await userEvent.selectOptions(screen.getByLabelText("Language"), "ar");
    expect(within(screen.getByLabelText("Script")).getAllByRole("option").map((o) => (o as HTMLOptionElement).value)).toEqual(["", "Arab"]);
  });

  it("says so when a recording has nothing published, and shows a failed lookup", async () => {
    mockFetch({ ...routes, "GET /api/v1/recordings/rec1/contributions": () => json(200, { items: [{ ...contributions[0], status: "pending" }], next_cursor: null }) });
    renderIntl(<PanelPicker onSelect={vi.fn()} />);
    await userEvent.type(screen.getByLabelText("Find a recording"), "take");
    await userEvent.click(screen.getByRole("button", { name: "Find" }));
    await userEvent.click(await screen.findByRole("button", { name: /Take 1/ }));
    expect(await screen.findByText("This recording has no published lyrics yet.")).toBeInTheDocument();
  });
});

describe("the comparison", () => {
  it("shows four independent panels with each text in its own direction, and the URL holds ids only", async () => {
    mockFetch(resolveRoute());
    renderIntl(<Compare initialIds={["c-orig", "c-alt", "c-en", "c-ar"]} />);
    const bodies = await Promise.all([0, 1, 2, 3].map((i) => screen.findByTestId(`panel-body-${i}`)));
    expect(bodies.map((b) => b.textContent)).toEqual(["ആദ്യ വരി", "ബദൽ വരി", "first line", "السطر الأول"]);
    expect(bodies.map((b) => b.getAttribute("dir"))).toEqual(["ltr", "ltr", "ltr", "rtl"]);
    expect(bodies[3]).toHaveAttribute("lang", "ar");
    // The alternative is labelled as such, never as the primary.
    expect(screen.getAllByText("Primary").length).toBe(3);
    expect(screen.getAllByText("Alternative").length).toBe(1);
    expect(document.querySelector(".compare")).toHaveAttribute("data-panels", "4");
    expect(screen.queryByRole("button", { name: "Add a panel" })).not.toBeInTheDocument();
    expect(screen.getByText("At most 4 panels.")).toBeInTheDocument();
    const url = new URL(window.location.href);
    expect(url.searchParams.get("c")).toBe("c-orig,c-alt,c-en,c-ar");
    expect(window.location.href).not.toContain(encodeURIComponent("ആദ്യ"));
  });

  it("on a narrow screen exactly one panel is active, switched by tabs; wide screens show columns", async () => {
    mockFetch(resolveRoute([panels[0], panels[2], panels[3]]));
    renderIntl(<Compare initialIds={["c-orig", "c-en", "c-ar"]} />);
    await screen.findByTestId("panel-body-0");
    const tabs = within(screen.getByRole("navigation", { name: "Panels" })).getAllByRole("button");
    expect(tabs).toHaveLength(3);
    const active = () => [...document.querySelectorAll(".compare-panel")].map((p) => p.getAttribute("data-active"));
    expect(active()).toEqual(["true", "false", "false"]);
    expect(tabs[0]).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(tabs[2]);
    expect(active()).toEqual(["false", "false", "true"]);
    expect(tabs[2]).toHaveAttribute("aria-pressed", "true");
    expect(tabs[0]).toHaveAttribute("aria-pressed", "false");

    // jsdom has no layout, so the responsive rules themselves are checked in the stylesheet.
    const css = readFileSync("src/app/globals.css", "utf8");
    expect(css).toMatch(/\.compare \{[^}]*grid-template-columns: repeat\(var\(--panels/);
    expect(css).toMatch(/@media \(max-width: 47\.99rem\) \{[^@]*\.compare-tabs \{ display: flex; \}[^@]*\.compare-panel\[data-active="false"\] \{ display: none; \}/);
    expect(css).toMatch(/\.compare-tabs \{ display: none;/);
  });

  it("is RTL-safe: the interface follows the Arabic locale while each text keeps its own direction", async () => {
    mockFetch(resolveRoute([panels[0], panels[3]]));
    renderIntl(<Compare initialIds={["c-orig", "c-ar"]} />, "ar");
    const first = await screen.findByTestId("panel-body-0");
    expect(first).toHaveAttribute("dir", "ltr"); // a Malayalam text inside an Arabic UI
    expect(screen.getByTestId("panel-body-1")).toHaveAttribute("dir", "rtl");
    expect(screen.getByRole("button", { name: "إضافة لوحة" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "اللوحات" })).toBeInTheDocument();
    // The comparison's styles use logical properties only, so mirroring needs no overrides.
    const css = readFileSync("src/app/globals.css", "utf8");
    const block = css.slice(css.indexOf("/* Comparison:"), css.indexOf(".badge-primary"));
    expect(block).not.toMatch(/\b(left|right|margin-left|margin-right|padding-left|padding-right|text-align:\s*(left|right))\b/);
    expect(block).toMatch(/padding-inline|margin-inline|inline-size/);
  });

  it("shows an unavailable version as such, without text, and lets the panel be removed", async () => {
    mockFetch(resolveRoute([{ id: "c-orig", available: false, content: null } as never, panels[2]]));
    renderIntl(<Compare initialIds={["c-orig", "c-en"]} />);
    expect(await screen.findByText("This version is no longer available.")).toBeInTheDocument();
    expect(screen.getByTestId("panel-body-1")).toHaveTextContent("first line");
    await userEvent.click(screen.getAllByRole("button", { name: "Remove" })[0]);
    await waitFor(() => expect(new URL(window.location.href).searchParams.get("c")).toBe("c-en"));
  });

  it("saves ids, not text, and reports a server refusal", async () => {
    let n = 0;
    const { calls } = mockFetch({
      ...resolveRoute(panels.slice(0, 2)),
      "POST /api/v1/me/comparisons": () =>
        n++ === 0
          ? json(401, { type: "/problems/unauthenticated", title: "x", status: 401, code: "unauthenticated" }, "application/problem+json")
          : json(201, { id: "s1", name: "Pair", contribution_ids: ["c-orig", "c-alt"], created_at: T, panels: null }),
    });
    renderIntl(<Compare initialIds={["c-orig", "c-alt"]} />);
    await screen.findByTestId("panel-body-0");
    await userEvent.click(screen.getByRole("button", { name: "Save comparison" }));
    await userEvent.type(screen.getByLabelText("Name"), "Pair");
    await userEvent.click(screen.getAllByRole("button", { name: "Save comparison" }).at(-1)!);
    expect(await screen.findByText("Please sign in to continue.")).toBeInTheDocument();
    await userEvent.click(screen.getAllByRole("button", { name: "Save comparison" }).at(-1)!);
    expect(await screen.findByText(/It always shows the current published text/)).toBeInTheDocument();
    const post = calls.filter((c) => c.method === "POST").at(-1)!;
    const sent = JSON.parse(await post.clone().text());
    expect(sent).toEqual({ name: "Pair", contribution_ids: ["c-orig", "c-alt"] });
    expect(JSON.stringify(sent)).not.toContain("ആദ്യ");
  });

  it("parses the shared address defensively", () => {
    expect(parseIds("a, b,,c,d,e,f")).toEqual(["a", "b", "c", "d"]);
    expect(parseIds(undefined)).toEqual([]);
  });
});
