import type { Chosen } from "@/components/contribute/name-picker";

export type Block = { key: string; language: string; script: string; type: "original" | "translation" | "transliteration"; body: string };

export type Prefill = {
  album: Chosen[];
  singers: Chosen[];
  composers: Chosen[];
  lyricists: Chosen[];
  raga: Chosen[];
  deity: Chosen[];
  tags: Chosen[];
  category?: "mainstream" | "devotional";
  form?: string;
  religion?: string;
  language?: string;
  /** Which fields came from the page the contributor was on, so they are shown but not editable. */
  locked: ReadonlySet<string>;
};

type Query = Record<string, string | string[] | undefined>;
const one = (q: Query, k: string) => (Array.isArray(q[k]) ? q[k]![0] : q[k])?.trim() || undefined;

/**
 * What a page that links here already knows. `album=<id>&album_name=Kireedam`, `singer=<id>&singer_name=…`
 * (also composer, lyricist), or plain values for raga, deity, tag, category, form, religion and language.
 * Whatever is given is locked: adding a song to an album means that album, not another one.
 */
export function parsePrefill(q: Query): Prefill {
  const ref = (k: string): Chosen[] => {
    const id = one(q, k);
    const name = one(q, `${k}_name`);
    return id && name ? [{ id, name, locked: true }] : [];
  };
  const text = (k: string): Chosen[] => {
    const v = one(q, k);
    return v ? [{ name: v, locked: true }] : [];
  };
  const category = one(q, "category") === "devotional" || one(q, "form") || one(q, "religion") || one(q, "deity") ? "devotional" : one(q, "category") === "mainstream" ? "mainstream" : undefined;
  const p: Prefill = {
    album: ref("album"),
    singers: ref("singer"),
    composers: ref("composer"),
    lyricists: ref("lyricist"),
    raga: text("raga"),
    deity: text("deity"),
    tags: text("tag"),
    category,
    form: one(q, "form"),
    religion: one(q, "religion"),
    language: one(q, "language"),
    locked: new Set<string>(),
  };
  const locked = new Set<string>();
  for (const [k, v] of Object.entries({ album: p.album, singers: p.singers, composers: p.composers, lyricists: p.lyricists, raga: p.raga, deity: p.deity, tags: p.tags }))
    if (v.length > 0) locked.add(k);
  for (const k of ["category", "form", "religion", "language"] as const) if (p[k]) locked.add(k);
  return { ...p, locked };
}

/** The address of the contribution form, pre-filled from a page's own metadata. */
export function contributeHref(params: Record<string, string | undefined>) {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v) q.set(k, v);
  const s = q.toString();
  return s ? `/contribute?${s}` : "/contribute";
}

const ref = (c: Chosen) => ({ id: c.id ?? null, name: c.name });
const list = (s: string) => s.split(/[,;،]/).map((x) => x.trim()).filter(Boolean);

export type Draft = {
  title: string;
  year: string;
  album: Chosen[];
  singers: Chosen[];
  composers: Chosen[];
  lyricists: Chosen[];
  category: "mainstream" | "devotional";
  religion: string;
  deity: Chosen[];
  raga: Chosen[];
  form: string;
  source: string;
  occasions: string;
  tags: Chosen[];
  youtube: string;
  blocks: Block[];
};

/** The request the API takes. Blocks without text are left out; devotional-only fields only go with devotional songs. */
export function buildSubmission(d: Draft, allowDuplicate = false) {
  const devotional = d.category === "devotional";
  const year = Number.parseInt(d.year, 10);
  return {
    title: d.title.trim(),
    year: Number.isFinite(year) ? year : null,
    album: d.album[0] ? ref(d.album[0]) : null,
    singers: d.singers.map(ref),
    composers: d.composers.map(ref),
    lyricists: d.lyricists.map(ref),
    category: d.category,
    religion: devotional ? d.religion : "",
    deity: devotional ? (d.deity[0]?.name ?? "") : "",
    raga: d.raga[0]?.name ?? "",
    devotional_form: devotional ? d.form : "",
    source_work: devotional ? d.source.trim() : "",
    occasions: devotional ? list(d.occasions) : [],
    tags: d.tags.map((t) => t.name),
    youtube: d.youtube.trim(),
    lyrics: d.blocks.filter((b) => b.body.trim() !== "").map((b) => ({ type: b.type, language: b.language, script: b.script, body: b.body })),
    allow_duplicate: allowDuplicate,
  };
}

/**
 * Where "add a song here" goes from the page of one value (a film, a singer, a raga…): the same form,
 * with that value filled in and fixed.
 */
export function prefillFor(facet: string, slug: string, name: string, id?: string | null) {
  const person = (k: string) => (id ? { [k]: id, [`${k}_name`]: name } : {});
  switch (facet) {
    case "album":
      return contributeHref(person("album"));
    case "singer":
    case "artist":
      return contributeHref(person("singer"));
    case "composer":
    case "lyricist":
      return contributeHref(person(facet));
    case "raga":
    case "deity":
    case "tag":
      return contributeHref({ [facet]: name });
    case "form":
    case "religion":
      return contributeHref({ [facet]: slug });
    case "category":
      return contributeHref({ category: slug });
    case "language":
      return contributeHref({ language: slug });
    default:
      return contributeHref({});
  }
}
