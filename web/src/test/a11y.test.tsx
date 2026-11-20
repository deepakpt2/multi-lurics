/**
 * Automated accessibility audit (axe-core rules for WCAG 2.1 A/AA) over the surfaces added for the
 * public and for moderation, in both locales. jsdom has no layout, so colour contrast and touch
 * target size are checked by hand in docs/operations.md instead of here.
 */
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import { screen } from "@testing-library/react";
import { json, mockFetch, renderIntl } from "@/test/utils";
import { Compare } from "@/components/compare/compare";
import { NoticeForm } from "@/components/legal/legal";
import { NoticeQueue } from "@/components/legal/legal";
import { SiteFooter } from "@/components/site-footer";
import { SiteHeader } from "@/components/site-header";

vi.mock("next/navigation", () => ({ useSearchParams: () => new URLSearchParams(""), useRouter: () => ({ push: vi.fn() }), usePathname: () => "/" }));

async function violations(container: Element) {
  const result = await axe.run(container, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "best-practice"] },
    rules: { "color-contrast": { enabled: false }, region: { enabled: false } },
  });
  return result.violations.map((v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(" ")).join(", ")})`);
}

const T = "2026-01-01T00:00:00Z";
const panel = (id: string, language: string, script: string, dir: string, body: string, primary = true) => ({
  id,
  available: true,
  content: { recording_id: "r", recording_title: "Take 1", composition_id: "w", composition_title: "Song", type: "original", language, script, dir,
    is_primary: primary, slot_size: 1, revision_id: `v-${id}`, revision_no: 1, body, revised_at: T },
});

describe.each(["en", "ar"] as const)("accessibility (%s)", (locale) => {
  it("the legal notice form has labelled controls and no violations", async () => {
    const { container } = renderIntl(<NoticeForm />, locale);
    expect(await violations(container)).toEqual([]);
  });

  it("an invalid notice announces its errors without violations", async () => {
    mockFetch({
      "POST /api/v1/legal/notices": () =>
        json(422, { type: "/problems/validation_failed", title: "x", status: 422, code: "validation_failed", request_id: "r", errors: [{ field: "contact_email", code: "invalid_email" }] }),
    });
    const { container } = renderIntl(<NoticeForm />, locale);
    const user = (await import("@testing-library/user-event")).default;
    await user.type(container.querySelector<HTMLInputElement>("input[name=contact_name]")!, "A");
    await user.type(container.querySelector<HTMLInputElement>("input[name=contact_email]")!, "a@b.co");
    await user.type(container.querySelector<HTMLTextAreaElement>("textarea[name=statement]")!, "A long enough statement here.");
    await user.click(container.querySelector("button[type=submit]")!);
    await screen.findByRole("alert");
    expect(await violations(container)).toEqual([]);
  });

  it("the comparison view with four panels, one in Arabic script", async () => {
    mockFetch({
      "GET /api/v1/comparisons/resolve": () =>
        json(200, { panels: [panel("a", "ml", "Mlym", "ltr", "ആദ്യ വരി"), panel("b", "ml", "Mlym", "ltr", "ബദൽ", false), panel("c", "en", "Latn", "ltr", "first"), panel("d", "ar", "Arab", "rtl", "السطر الأول")] }),
    });
    const { container } = renderIntl(<Compare initialIds={["a", "b", "c", "d"]} />, locale);
    await screen.findByTestId("panel-body-3");
    expect(await violations(container)).toEqual([]);
  });

  it("the moderation queue for legal notices", async () => {
    mockFetch({
      "GET /api/v1/moderation/legal/notices": () =>
        json(200, { items: [{ id: "n1", reference: "LN-ABC", kind: "copyright", contribution_id: "c1", contact_name: "Rights Holder", contact_email: "o@example.org", statement: "My work.", status: "open", decided_by: null, decided_at: null, decision_note: null, created_at: T }], next_cursor: null }),
    });
    const { container } = renderIntl(<NoticeQueue />, locale);
    await screen.findByTestId("notice");
    expect(await violations(container)).toEqual([]);
  });

  it("header and footer navigation", async () => {
    const { container } = renderIntl(
      <>
        <SiteHeader />
        <SiteFooter />
      </>,
      locale,
    );
    expect(await violations(container)).toEqual([]);
  });
});
