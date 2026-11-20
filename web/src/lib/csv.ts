/**
 * A small CSV reader for the bulk import: quoted cells (with commas, quotes and line breaks inside, which
 * lyrics need), CRLF or LF, a leading byte-order mark, and a comma, semicolon or tab between cells.
 */
export function parseCsv(input: string): string[][] {
  const text = input.replace(/^﻿/, "");
  const first = text.slice(0, text.search(/\r?\n|$/));
  const count = (c: string) => first.split(c).length - 1;
  const delimiter = [",", ";", "\t"].reduce((best, c) => (count(c) > count(best) ? c : best), ",");
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        cell += '"';
        i++;
      } else if (c === '"') quoted = false;
      else cell += c;
    } else if (c === '"' && cell === "") quoted = true;
    else if (c === delimiter) {
      row.push(cell);
      cell = "";
    }
    else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(cell);
      rows.push(row);
      row = [];
      cell = "";
    } else cell += c;
  }
  if (cell !== "" || row.length > 0) {
    row.push(cell);
    rows.push(row);
  }
  return rows.filter((r) => r.some((c) => c.trim() !== ""));
}

/** One cell, quoted when it has to be. */
export const csvCell = (v: string) => (/[",\r\n;\t]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
export const toCsv = (rows: string[][]) => `﻿${rows.map((r) => r.map(csvCell).join(",")).join("\r\n")}\r\n`;
