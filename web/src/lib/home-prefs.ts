/** Which home sections a person shows and in what order. Kept in this browser, per signed-in user. */
export const SECTION_KEYS = ["editors_picks", "popular_songs", "new_songs", "popular_albums", "new_albums", "popular_artists"] as const;
export type SectionKey = (typeof SECTION_KEYS)[number];
export type Prefs = { order: SectionKey[]; hidden: SectionKey[] };

export const DEFAULT_PREFS: Prefs = { order: [...SECTION_KEYS], hidden: [] };

const isKey = (k: unknown): k is SectionKey => typeof k === "string" && (SECTION_KEYS as readonly string[]).includes(k);
const unique = <T,>(xs: T[]) => xs.filter((x, i) => xs.indexOf(x) === i);

/** Whatever was stored, made valid: unknown or repeated keys dropped, sections added since go last (visible). */
export function normalize(raw: unknown): Prefs {
  const r = (typeof raw === "object" && raw !== null ? raw : {}) as { order?: unknown; hidden?: unknown };
  const order = unique((Array.isArray(r.order) ? r.order : []).filter(isKey));
  for (const k of SECTION_KEYS) if (!order.includes(k)) order.push(k);
  const hidden = unique((Array.isArray(r.hidden) ? r.hidden : []).filter(isKey));
  return { order, hidden };
}

/** One entry per person (or one for visitors who are not signed in). */
export const storageKey = (userId?: string | null) => `lyrics.home.v1:${userId ?? "guest"}`;

export function loadPrefs(userId?: string | null): Prefs {
  try {
    const raw = window.localStorage.getItem(storageKey(userId));
    return raw ? normalize(JSON.parse(raw)) : DEFAULT_PREFS;
  } catch {
    return DEFAULT_PREFS; // storage blocked or corrupt: the default layout
  }
}

export function savePrefs(userId: string | null | undefined, prefs: Prefs) {
  try {
    window.localStorage.setItem(storageKey(userId), JSON.stringify(prefs));
  } catch {
    // Not persisted (private window, quota): the layout still works for this visit.
  }
}

export const visible = (p: Prefs) => p.order.filter((k) => !p.hidden.includes(k));

/** Swaps a section with its nearest visible neighbour in `dir`. */
export function move(p: Prefs, key: SectionKey, dir: -1 | 1): Prefs {
  const vis = visible(p);
  const at = vis.indexOf(key);
  const other = vis[at + dir];
  if (at < 0 || other === undefined) return p;
  const order = [...p.order];
  const [i, j] = [order.indexOf(key), order.indexOf(other)];
  [order[i], order[j]] = [order[j], order[i]];
  return { ...p, order };
}

/** Drag and drop: put `key` where `target` is. */
export function moveTo(p: Prefs, key: SectionKey, target: SectionKey): Prefs {
  if (key === target) return p;
  const order = p.order.filter((k) => k !== key);
  order.splice(order.indexOf(target) + (p.order.indexOf(key) < p.order.indexOf(target) ? 1 : 0), 0, key);
  return { ...p, order };
}

export const hide = (p: Prefs, key: SectionKey): Prefs => ({ ...p, hidden: unique([...p.hidden, key]) });
export const show = (p: Prefs, key: SectionKey): Prefs => ({ ...p, hidden: p.hidden.filter((k) => k !== key) });
