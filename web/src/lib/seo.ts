import type { components } from "@/api/schema";

export type PublicPage = components["schemas"]["PublicPage"];
export type PageRef = components["schemas"]["PageRef"];
export type Scope = "song" | "artist" | "recording";
export type Fetched = { kind: "page"; page: PublicPage } | { kind: "moved"; to: PageRef } | { kind: "missing" };

/** Server-side only: where the API lives (the browser goes through the same-origin proxy instead). */
const apiOrigin = () => process.env.API_ORIGIN ?? "http://127.0.0.1:8787";

/** Public origin used in canonical URLs, hreflang and sitemaps. */
export function siteUrl(): string {
  return (process.env.SITE_URL ?? process.env.LYRICS_APP_BASE_URL ?? "http://localhost:3000").replace(/\/+$/, "");
}

export const SCOPES: readonly Scope[] = ["song", "artist", "recording"];

/** The cache tag the worker invalidates when this address's page changes (see `page_tags`). */
export const slugTag = (scope: string, locale: string, slug: string) => `slug:${scope}:${locale}:${slug}`;

export function decodeSlug(raw: string): string {
  try {
    return decodeURIComponent(raw).normalize("NFC");
  } catch {
    return raw;
  }
}

/** Absolute, percent-encoded URL of a page. Query parameters are never part of it. */
export const pageUrl = (ref: PageRef, scope: string) => `${siteUrl()}${pagePath(ref, scope)}`;
export const pagePath = (ref: PageRef, scope: string) => `/${ref.locale}/${scope}/${encodeURIComponent(ref.slug)}`;

/**
 * Fetches a public page through the API. Cached (ISR) and tagged by address so a publish, rename
 * or unpublish can invalidate exactly the right pages. Unpublished content comes back 404 and
 * is never stored: the API marks it `no-store`, and only successful answers are kept.
 */
export async function fetchPage(scope: Scope, locale: string, slug: string): Promise<Fetched> {
  const url = `${apiOrigin()}/api/v1/pages/${scope}/${encodeURIComponent(slug)}?locale=${encodeURIComponent(locale)}`;
  const res = await fetch(url, { redirect: "manual", next: { revalidate: 300, tags: [slugTag(scope, locale, slug)] } });
  if (res.status === 301) return { kind: "moved", to: (await res.json()) as PageRef };
  if (res.status === 404) return { kind: "missing" };
  if (!res.ok) throw new Error(`page API answered ${res.status}`);
  return { kind: "page", page: (await res.json()) as PublicPage };
}

type SitemapItem = components["schemas"]["SitemapItem"];
export type Sitemap = { total: number; page_size: number; pages: number; items: SitemapItem[] };

export async function fetchSitemap(page: number): Promise<Sitemap> {
  const res = await fetch(`${apiOrigin()}/api/v1/sitemap?page=${page}`, { next: { revalidate: 300, tags: ["sitemap"] } });
  if (!res.ok) throw new Error(`sitemap API answered ${res.status}`);
  return (await res.json()) as Sitemap;
}

/** JSON for a `<script type="application/ld+json">`: nothing in it can close the tag. */
export const safeJson = (value: unknown) =>
  JSON.stringify(value).replace(/</g, "\\u003c").replace(/\u2028/g, "\\u2028").replace(/\u2029/g, "\\u2029");

const iso8601 = (ms: number) => {
  const s = Math.round(ms / 1000);
  return `PT${Math.floor(s / 3600) ? `${Math.floor(s / 3600)}H` : ""}${Math.floor((s % 3600) / 60)}M${s % 60}S`;
};

/** schema.org description of the page: MusicComposition, MusicRecording or the artist. */
export function jsonLd(p: PublicPage): Record<string, unknown> {
  const url = pageUrl(p.canonical, p.scope);
  const person = (c: PublicPage["credits"][number]["artist"]) => ({
    "@type": "Person",
    name: c.label.text,
    ...(c.page ? { url: pageUrl(c.page, "artist") } : {}),
  });
  const credited = (roles: string[]) => p.credits.filter((c) => roles.includes(c.role)).map((c) => person(c.artist));
  const inLanguage = p.lyrics[0]?.language;
  if (p.scope === "recording") {
    return {
      "@context": "https://schema.org",
      "@type": "MusicRecording",
      name: p.title.text,
      url,
      inLanguage,
      ...(p.duration_ms ? { duration: iso8601(p.duration_ms) } : {}),
      ...(p.code ? { isrcCode: p.code } : {}),
      byArtist: credited(["performer", "featured_performer", "primary_artist"]),
      ...(p.parent?.page ? { recordingOf: { "@type": "MusicComposition", name: p.parent.label.text, url: pageUrl(p.parent.page, "song") } } : {}),
    };
  }
  if (p.scope === "song") {
    return {
      "@context": "https://schema.org",
      "@type": "MusicComposition",
      name: p.title.text,
      url,
      inLanguage,
      ...(p.code ? { iswcCode: p.code } : {}),
      composer: credited(["composer"]),
      lyricist: credited(["lyricist"]),
      ...(p.lyrics[0] ? { lyrics: { "@type": "CreativeWork", text: p.lyrics[0].body, inLanguage: p.lyrics[0].language } } : {}),
      recordedAs: p.recordings.filter((r) => r.page).map((r) => ({ "@type": "MusicRecording", name: r.label.text, url: pageUrl(r.page!, "recording") })),
    };
  }
  return { "@context": "https://schema.org", "@type": p.kind === "group" ? "MusicGroup" : "Person", name: p.title.text, url };
}

/** First lines of the primary lyric (or the credits) as a plain description. */
export function describe(p: PublicPage, max = 160): string | undefined {
  const text = (p.lyrics.find((l) => l.is_primary) ?? p.lyrics[0])?.body.replace(/\s+/g, " ").trim();
  if (!text) return undefined;
  return [...text].length > max ? `${[...text].slice(0, max - 1).join("").trimEnd()}…` : text;
}

export const xmlEscape = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&apos;");
