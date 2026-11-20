import type { components } from "@/api/schema";

export type FacetKey = components["schemas"]["BrowseFacet"];
export type BrowseFilter = components["schemas"]["BrowseFilter"];
export type BrowseSong = components["schemas"]["BrowseSong"];
export type SongsResponse = components["schemas"]["BrowseSongsResponse"];
export type FacetResponse = components["schemas"]["BrowseFacetResponse"];
export type Suggestion = components["schemas"]["BrowseSuggestion"];

/** Every kind of metadata, in the order they are offered. The generated `Facet` type forces this list to stay complete. */
export const FACET_KEYS = [
  "album", "artist", "singer", "lyricist", "composer", "raga", "deity", "religion", "category", "form", "language", "tag",
] as const satisfies readonly FacetKey[];
type Missing = Exclude<FacetKey, (typeof FACET_KEYS)[number]>;
const _complete: Missing extends never ? true : never = true;
void _complete;

export const isFacet = (s: string): s is FacetKey => (FACET_KEYS as readonly string[]).includes(s);

export const PAGE_SIZES = [10, 20, 50] as const;
export const DEFAULT_PAGE_SIZE = 20;

export type Selected = { facet: FacetKey; slug: string };
export type Sort = "newest" | "title" | "album";
export type BrowseState = { filters: Selected[]; page: number; pageSize: number; sort: Sort };

type Params = URLSearchParams | Record<string, string | string[] | undefined>;

function entries(p: Params): [string, string][] {
  if (p instanceof URLSearchParams) return [...p.entries()];
  return Object.entries(p).flatMap(([k, v]) => (v === undefined ? [] : (Array.isArray(v) ? v : [v]).map((x) => [k, x] as [string, string])));
}

const positive = (s: string | undefined, fallback: number) => {
  const n = Number(s);
  return Number.isInteger(n) && n >= 1 ? n : fallback;
};

/** The state a URL describes. Unknown parameters are ignored; repeated filters are kept once, in order. */
export function parseState(p: Params): BrowseState {
  const all = entries(p);
  const first = (k: string) => all.find(([key]) => key === k)?.[1];
  const filters: Selected[] = [];
  for (const [k, v] of all) {
    if (isFacet(k) && v && !filters.some((f) => f.facet === k && f.slug === v)) filters.push({ facet: k, slug: v });
  }
  const size = positive(first("page_size"), DEFAULT_PAGE_SIZE);
  return {
    filters,
    page: positive(first("page"), 1),
    pageSize: (PAGE_SIZES as readonly number[]).includes(size) ? size : DEFAULT_PAGE_SIZE,
    sort: first("sort") === "title" ? "title" : first("sort") === "album" ? "album" : "newest",
  };
}

/** The one canonical query string for a state: defaults are left out, so equal states give equal addresses. */
export function toQuery(s: Partial<BrowseState> & Pick<BrowseState, "filters">, extra: Record<string, string> = {}): string {
  const q = new URLSearchParams();
  for (const f of s.filters) q.append(f.facet, f.slug);
  if (s.sort && s.sort !== "newest") q.set("sort", s.sort);
  if (s.pageSize && s.pageSize !== DEFAULT_PAGE_SIZE) q.set("page_size", String(s.pageSize));
  if (s.page && s.page > 1) q.set("page", String(s.page));
  for (const [k, v] of Object.entries(extra)) if (v) q.set(k, v);
  return q.toString();
}

export const browseHref = (s: Partial<BrowseState> & Pick<BrowseState, "filters">, path = "/browse", extra: Record<string, string> = {}) => {
  const q = toQuery(s, extra);
  return q ? `${path}?${q}` : path;
};

/** The address of one value's page: a singer, a film (album), a raga… with its songs grouped by album. */
export const itemHref = (facet: FacetKey, slug: string) => `/browse/${facet}/${encodeURIComponent(slug)}`;

/** Adds a filter (page back to 1). Choosing something already chosen changes nothing. */
export function withFilter(s: BrowseState, f: Selected): BrowseState {
  return s.filters.some((x) => x.facet === f.facet && x.slug === f.slug) ? s : { ...s, filters: [...s.filters, f], page: 1 };
}

export function withoutFilter(s: BrowseState, index: number): BrowseState {
  return { ...s, filters: s.filters.filter((_, i) => i !== index), page: 1 };
}

/** Page numbers to show: first, last, and a window around the current page, with `null` for a gap. */
export function pageWindow(page: number, pages: number): (number | null)[] {
  const keep = new Set([1, pages, page - 1, page, page + 1].filter((n) => n >= 1 && n <= pages));
  const sorted = [...keep].sort((a, b) => a - b);
  const out: (number | null)[] = [];
  sorted.forEach((n, i) => {
    if (i > 0 && n - sorted[i - 1] > 1) out.push(null);
    out.push(n);
  });
  return out;
}

const apiOrigin = () => process.env.API_ORIGIN ?? "http://127.0.0.1:8787";

export type Loaded<T> = { ok: true; data: T } | { ok: false; status: number };

/** Server-side read of a browse endpoint. A failure is a value, so the page can show a retry instead of crashing. */
export async function fetchBrowse<T>(path: string, query: string, locale: string): Promise<Loaded<T>> {
  const q = new URLSearchParams(query);
  q.set("lang", locale);
  try {
    const res = await fetch(`${apiOrigin()}/api/v1/browse/${path}?${q}`, { next: { revalidate: 30 } });
    if (!res.ok) return { ok: false, status: res.status };
    return { ok: true, data: (await res.json()) as T };
  } catch {
    return { ok: false, status: 0 };
  }
}
