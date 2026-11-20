import { describe, expect, it } from "vitest";
import { fromLocalInput, parseMarkdownFile, safeImageSrc, toLocalInput } from "./articles";

describe("parseMarkdownFile", () => {
  it("reads front matter", () => {
    const r = parseMarkdownFile('---\ntitle: "Onam 2026"\nsummary: Celebrate\nslug: onam-2026\n---\n\nHello\n');
    expect(r).toEqual({ title: "Onam 2026", summary: "Celebrate", slug: "onam-2026", body: "Hello\n" });
  });
  it("takes a leading heading as the title", () => {
    expect(parseMarkdownFile("# Vishu\r\n\r\nText")).toEqual({ title: "Vishu", body: "Text" });
  });
  it("leaves plain text alone", () => {
    expect(parseMarkdownFile("Just text\n\n# Later heading")).toEqual({ body: "Just text\n\n# Later heading" });
  });
});

describe("dates", () => {
  it("round-trips local input values", () => {
    const iso = fromLocalInput("2026-08-26T09:30");
    expect(iso).not.toBeNull();
    expect(toLocalInput(iso)).toBe("2026-08-26T09:30");
    expect(fromLocalInput("")).toBeNull();
    expect(toLocalInput(null)).toBe("");
  });
});

describe("safeImageSrc", () => {
  it("allows our media and https only", () => {
    expect(safeImageSrc("/api/v1/media/abc/960")).toBe(true);
    expect(safeImageSrc("https://example.com/a.jpg")).toBe(true);
    expect(safeImageSrc("http://example.com/a.jpg")).toBe(false);
    expect(safeImageSrc("javascript:alert(1)")).toBe(false);
    expect(safeImageSrc(undefined)).toBe(false);
  });
});
