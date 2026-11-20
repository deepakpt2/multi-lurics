/** Pure rules behind the side-by-side lyric columns; the component only draws them. */
import type { components } from "@/api/schema";

export type Lyric = components["schemas"]["PageLyric"];
export const TYPES = ["original", "translation", "transliteration"] as const;
export type LyricType = (typeof TYPES)[number];
export type Pane = { key: string; type: LyricType; lyricId: string | null };

export const isType = (s: string): s is LyricType => (TYPES as readonly string[]).includes(s);

let counter = 0;
const nextKey = () => `pane-${++counter}`;

/** The lyrics of one type, in the order the page lists them (the primary original first). */
export const ofType = (lyrics: Lyric[], type: LyricType) => lyrics.filter((l) => l.type === type);

/** The columns to start with: the ones named in the address, else the song's main lyric. Unknown ids are ignored. */
export function initialPanes(lyrics: Lyric[], wanted?: string[]): Pane[] {
  const picked = (wanted ?? []).map((id) => lyrics.find((l) => l.id === id)).filter((l): l is Lyric => !!l);
  const first = picked.length > 0 ? picked : lyrics.slice(0, 1);
  if (first.length === 0) return [{ key: nextKey(), type: "original", lyricId: null }];
  return first.map((l) => ({ key: nextKey(), type: l.type as LyricType, lyricId: l.id }));
}

/** A new, empty column. Its type is the first one that still has something not shown, so the picker starts useful. */
export function emptyPane(lyrics: Lyric[], panes: Pane[]): Pane {
  const shown = new Set(panes.map((p) => p.lyricId));
  const type = TYPES.find((t) => ofType(lyrics, t).some((l) => !shown.has(l.id))) ?? "translation";
  return { key: nextKey(), type, lyricId: null };
}

/** Changing a column's type keeps its lyric only when that lyric is of the new type. */
export function withType(lyrics: Lyric[], pane: Pane, type: LyricType): Pane {
  const keep = lyrics.find((l) => l.id === pane.lyricId && l.type === type);
  return { ...pane, type, lyricId: keep?.id ?? null };
}

/** The address a set of columns is shared as: `?l=id,id`. Columns without a lyric yet are left out. */
export const idsParam = (panes: Pane[]) => panes.flatMap((p) => (p.lyricId ? [p.lyricId] : [])).join(",");
export const parseIds = (v: string | string[] | undefined) =>
  (Array.isArray(v) ? v.join(",") : (v ?? "")).split(",").map((s) => s.trim()).filter(Boolean).slice(0, 12);

/** Reading preferences, kept in the browser. */
export const SIZES = [0.9, 1.05, 1.2, 1.4, 1.7, 2.1, 2.6] as const;
export const THEMES = ["light", "sepia", "dark"] as const;
export type Theme = (typeof THEMES)[number];
export type Reading = { size: number; theme: Theme };
export const READING_KEY = "lyrics.reading.v1";
export const DEFAULT_READING: Reading = { size: 2, theme: "light" };

export function loadReading(): Reading {
  try {
    const v = JSON.parse(window.localStorage.getItem(READING_KEY) ?? "null") as Partial<Reading> | null;
    return {
      size: Number.isInteger(v?.size) && v!.size! >= 0 && v!.size! < SIZES.length ? v!.size! : DEFAULT_READING.size,
      theme: (THEMES as readonly string[]).includes(v?.theme ?? "") ? (v!.theme as Theme) : DEFAULT_READING.theme,
    };
  } catch {
    return DEFAULT_READING;
  }
}

export function saveReading(r: Reading) {
  try {
    window.localStorage.setItem(READING_KEY, JSON.stringify(r));
  } catch {
    // Private windows and blocked storage: the choice just lasts until the page closes.
  }
}
