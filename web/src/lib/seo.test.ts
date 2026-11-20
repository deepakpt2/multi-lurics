import { describe, expect, it, vi } from "vitest";
import { decodeSlug, describe as describePage, fetchPage, jsonLd, pagePath, pageUrl, safeJson, slugTag, xmlEscape, type PublicPage } from "./seo";

const disp = (text: string) => ({ text, description: null, language: null, script: null, dir: "ltr" as const, fallback: false, resolution: "base" as const });
const link = (id: string, label: string, scope: string, slug?: string) => ({ id, label: disp(label), scope, page: slug ? { locale: "en", slug } : null });
const base = (over: Partial<PublicPage>): PublicPage =>
  ({
    scope: "song", id: "c1", locale: "en", canonical: { locale: "en", slug: "kaatrin-mozhi" }, alternates: [], x_default: { locale: "en", slug: "kaatrin-mozhi" },
    title: disp("Kaatrin Mozhi"), year: null, code: null, kind: null, detail: null, duration_ms: null, category: "mainstream", raga: null, religion: null, deity: null, tags: [], parent: null, credits: [], songs: [], recordings: [], lyrics: [],
    updated_at: "2026-01-01T00:00:00Z", ...over,
  }) as PublicPage;

describe("addresses", () => {
  it("percent-encodes Unicode slugs and never carries a query string", () => {
    expect(pagePath({ locale: "ar", slug: "كاترين-موزي" }, "song")).toBe("/ar/song/%D9%83%D8%A7%D8%AA%D8%B1%D9%8A%D9%86-%D9%85%D9%88%D8%B2%D9%8A");
    expect(pageUrl({ locale: "en", slug: "a-b" }, "artist")).toBe("http://localhost:3000/en/artist/a-b");
  });

  it("round-trips encoded and decomposed slugs to the NFC form the server stores", () => {
    const slug = "كاترين-موزي";
    expect(decodeSlug(encodeURIComponent(slug))).toBe(slug);
    expect(decodeSlug(encodeURIComponent("café"))).toBe("café");
    expect(decodeSlug("%E0%A4%A")).toBe("%E0%A4%A"); // malformed input is passed through, not thrown
    expect(slugTag("song", "ar", slug)).toBe(`slug:song:ar:${slug}`);
  });
});

describe("structured data", () => {
  it("cannot be used to break out of the script tag", () => {
    const out = safeJson({ name: "</script><script>alert(1)</script>", sep: " " });
    expect(out).not.toContain("</script>");
    expect(out).not.toContain(" ");
    expect(JSON.parse(out).name).toBe("</script><script>alert(1)</script>");
  });

  it("publishes raga, deity and tags of a song as keywords", () => {
    const ld = jsonLd(base({ category: "devotional", raga: "Mohanam", deity: "Hanuman", religion: "hindu", tags: ["bhajan"] }));
    expect(ld).toMatchObject({ "@type": "MusicComposition", keywords: "Mohanam, Hanuman, bhajan" });
    expect(jsonLd(base({}))).not.toHaveProperty("keywords");
  });

  it("describes a song as MusicComposition with credits and recordings", () => {
    const ld = jsonLd(
      base({
        code: "T0345246800",
        credits: [
          { artist: link("a1", "A. R. Rahman", "artist", "a-r-rahman"), role: "composer" },
          { artist: link("a2", "Vairamuthu", "artist"), role: "lyricist" },
        ],
        recordings: [link("r1", "Take 1", "recording", "take-1")],
        lyrics: [{ id: "l1", recording_id: "r1", type: "original", language: "ml", script: "Mlym", dir: "ltr", is_primary: true, body: "വരി", revised_at: "2026-01-01T00:00:00Z" }],
      }),
    );
    expect(ld).toMatchObject({
      "@type": "MusicComposition", name: "Kaatrin Mozhi", url: "http://localhost:3000/en/song/kaatrin-mozhi", inLanguage: "ml", iswcCode: "T0345246800",
      composer: [{ "@type": "Person", name: "A. R. Rahman", url: "http://localhost:3000/en/artist/a-r-rahman" }],
      lyricist: [{ "@type": "Person", name: "Vairamuthu" }],
      recordedAs: [{ "@type": "MusicRecording", name: "Take 1", url: "http://localhost:3000/en/recording/take-1" }],
    });
    expect((ld.lyricist as object[])[0]).not.toHaveProperty("url");
  });

  it("describes a recording with ISO 8601 duration, ISRC and its song", () => {
    const ld = jsonLd(
      base({ scope: "recording", title: disp("Take 1"), canonical: { locale: "en", slug: "take-1" }, duration_ms: 245000, code: "USRC17607839", parent: link("c1", "Kaatrin Mozhi", "song", "kaatrin-mozhi"), credits: [{ artist: link("a1", "Singer", "artist", "singer"), role: "performer" }] }),
    );
    expect(ld).toMatchObject({ "@type": "MusicRecording", duration: "PT4M5S", isrcCode: "USRC17607839", byArtist: [{ name: "Singer" }], recordingOf: { "@type": "MusicComposition", name: "Kaatrin Mozhi" } });
  });

  it("describes an artist by kind", () => {
    expect(jsonLd(base({ scope: "artist", kind: "group", canonical: { locale: "en", slug: "band" } }))["@type"]).toBe("MusicGroup");
    expect(jsonLd(base({ scope: "artist", kind: "person" }))["@type"]).toBe("Person");
  });

  it("builds a description from the primary lyric, trimmed on a character boundary", () => {
    const lyric = (body: string, is_primary: boolean) => ({ id: body, recording_id: "r", type: "original", language: "ml", script: "Mlym", dir: "ltr" as const, is_primary, body, revised_at: "" });
    expect(describePage(base({ lyrics: [lyric("other", false), lyric("first  line\nsecond", true)] }))).toBe("first line second");
    const long = describePage(base({ lyrics: [lyric("കാ".repeat(300), true)] }), 20)!;
    expect([...long]).toHaveLength(20);
    expect(long.endsWith("…")).toBe(true);
    expect(describePage(base({}))).toBeUndefined();
    expect(xmlEscape(`a&b<c>"d"'e'`)).toBe("a&amp;b&lt;c&gt;&quot;d&quot;&apos;e&apos;");
  });
});

describe("fetching", () => {
  const reply = (status: number, body?: unknown) => vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(body ? JSON.stringify(body) : null, { status, headers: { "content-type": "application/json" } }));

  it("maps the API's answers and tags the request by address for invalidation", async () => {
    const spy = reply(200, base({}));
    expect((await fetchPage("song", "en", "kaatrin-mozhi")).kind).toBe("page");
    const [url, init] = spy.mock.calls[0] as [string, RequestInit & { next?: { tags?: string[]; revalidate?: number } }];
    expect(url).toBe("http://127.0.0.1:8787/api/v1/pages/song/kaatrin-mozhi?locale=en");
    expect(init.redirect).toBe("manual");
    expect(init.next).toEqual({ revalidate: 300, tags: ["slug:song:en:kaatrin-mozhi"] });

    reply(301, { locale: "en", slug: "new" });
    expect(await fetchPage("song", "en", "old")).toEqual({ kind: "moved", to: { locale: "en", slug: "new" } });
    reply(404);
    expect(await fetchPage("song", "en", "hidden")).toEqual({ kind: "missing" });
    reply(500);
    await expect(fetchPage("song", "en", "x")).rejects.toThrow("500");
  });

  it("encodes the slug in the API call", async () => {
    const spy = reply(404);
    await fetchPage("song", "ar", "كاترين-موزي");
    expect((spy.mock.calls[0] as [string])[0]).toContain("/pages/song/%D9%83");
  });
});
