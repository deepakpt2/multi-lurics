import type { components } from "@/api/schema";
import { parseCsv, toCsv } from "./csv";

export type ImportRow = components["schemas"]["ImportRow"];
export type ImportResult = components["schemas"]["ImportRowResult"];

/** The columns of the import file, in the order the template shows them. */
export const COLUMNS = [
  "title", "language", "script", "lyrics", "album", "album_kind", "year", "singers", "composers", "lyricists", "category",
  "religion", "deity", "raga", "devotional_form", "source_work", "occasions", "tags", "youtube",
] as const;
export type Column = (typeof COLUMNS)[number];

const LISTS: ReadonlySet<Column> = new Set(["singers", "composers", "lyricists", "occasions", "tags"]);

/** Other names a column goes by in spreadsheets people already have. */
const ALIASES: Record<string, Column> = {
  song: "title", song_title: "title", name: "title",
  lang: "language", lyrics_language: "language",
  lyric: "lyrics", text: "lyrics", body: "lyrics", lyrics_text: "lyrics",
  film: "album", movie: "album", album_name: "album", film_name: "album",
  type: "album_kind", kind: "album_kind",
  singer: "singers", artist: "singers", artists: "singers", vocals: "singers",
  composer: "composers", music: "composers", music_director: "composers",
  lyricist: "lyricists", writer: "lyricists", written_by: "lyricists", lyrics_by: "lyricists",
  tag: "tags", occasion: "occasions", form: "devotional_form", source: "source_work",
  video: "youtube", youtube_link: "youtube", video_url: "youtube", url: "youtube",
};

const normalize = (h: string) => h.trim().toLowerCase().replace(/[\s-]+/g, "_");
export const columnOf = (header: string): Column | null => {
  const k = normalize(header);
  return (COLUMNS as readonly string[]).includes(k) ? (k as Column) : (ALIASES[k] ?? null);
};

/** People and tags: separated by `;`, `|` or a line break (a comma is part of a name such as "Sharma, R."). */
const list = (v: string) => v.split(/[;|\n]/).map((x) => x.trim()).filter(Boolean);

export type Parsed = {
  rows: ImportRow[];
  /** Columns that were recognised, and header names that were not (they are ignored). */
  columns: Column[];
  ignored: string[];
};

/** Turns the text of a spreadsheet export into rows the API takes. */
export function parseImport(text: string): Parsed {
  const [header = [], ...body] = parseCsv(text);
  const map = header.map((h) => columnOf(h));
  const columns = [...new Set(map.filter((c): c is Column => c !== null))];
  const ignored = header.filter((_, i) => map[i] === null && header[i].trim() !== "");
  const film = header.some((h) => ["film", "movie", "film_name"].includes(normalize(h)));
  const rows = body.map((cells) => {
    const row: Record<string, unknown> = {};
    map.forEach((col, i) => {
      if (!col) return;
      const v = (cells[i] ?? "").trim();
      if (col === "year") {
        const n = Number.parseInt(v, 10);
        if (v !== "") row.year = Number.isFinite(n) ? n : -1;
      } else if (col === "lyrics") row.lyrics = (cells[i] ?? "").replace(/\r\n?/g, "\n").trim();
      else if (LISTS.has(col)) row[col] = list(v);
      else row[col] = col === "category" ? v.toLowerCase() : v;
    });
    if (film && row.album && !row.album_kind) row.album_kind = "soundtrack";
    return row as ImportRow;
  });
  return { rows, columns, ignored };
}

/** A starting file: the header, one film song, one devotional piece. */
export function template(): string {
  const film = ["Kaatrin Mozhi", "ml", "", "ഒന്നാം വരി\nരണ്ടാം വരി", "Kireedam", "soundtrack", "1989", "M. G. Sreekumar; K. S. Chithra", "Johnson", "Kaithapram", "mainstream", "", "", "", "", "", "", "film; duet", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"];
  const bhajan = ["Hanuman Chalisa", "hi", "", "श्रीगुरु चरन सरोज रज\nनिज मनु मुकुरु सुधारि", "", "", "", "", "", "Tulsidas", "devotional", "hindu", "Hanuman", "", "chalisa", "Ramcharitmanas", "Tuesday; Hanuman Jayanti", "bhajan; morning", ""];
  return toCsv([[...COLUMNS], film, bhajan]);
}
