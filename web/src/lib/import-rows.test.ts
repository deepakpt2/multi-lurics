import { describe, expect, it } from "vitest";
import { parseCsv } from "./csv";
import { COLUMNS, columnOf, parseImport, template } from "./import-rows";

describe("csv", () => {
  it("reads quotes, doubled quotes, line breaks inside cells, CRLF and a byte-order mark", () => {
    const rows = parseCsv('﻿a,b,c\r\n"x, y","say ""hi""","line1\nline2"\r\n1,2,3');
    expect(rows).toEqual([["a", "b", "c"], ["x, y", 'say "hi"', "line1\nline2"], ["1", "2", "3"]]);
  });
  it("finds the delimiter and skips blank lines", () => {
    expect(parseCsv("a;b\n1;2\n\n3;4\n")).toEqual([["a", "b"], ["1", "2"], ["3", "4"]]);
    expect(parseCsv("a\tb\n1\t2")).toEqual([["a", "b"], ["1", "2"]]);
  });
});

describe("import file", () => {
  it("knows the usual names for each column", () => {
    expect(columnOf("Song")).toBe("title");
    expect(columnOf(" Music Director ")).toBe("composers");
    expect(columnOf("Written-By")).toBe("lyricists");
    expect(columnOf("nonsense")).toBeNull();
  });

  it("turns a sheet into rows: lists on ; or |, numbers, multi-line lyrics, ignored columns reported", () => {
    const p = parseImport('Song,Film,Singer,Year,Lyrics,Notes\n"Kannum Kannum",Kireedam,"A; B | C",1989,"one\ntwo",x\n');
    expect(p.columns).toEqual(["title", "album", "singers", "year", "lyrics"]);
    expect(p.ignored).toEqual(["Notes"]);
    expect(p.rows).toEqual([{ title: "Kannum Kannum", album: "Kireedam", album_kind: "soundtrack", singers: ["A", "B", "C"], year: 1989, lyrics: "one\ntwo" }]);
  });

  it("a year that is not a number is sent as one the server refuses, not dropped", () => {
    expect(parseImport("title,year\nX,abc").rows[0].year).toBe(-1);
  });

  it("the template has every column and parses back into its two example songs", () => {
    const p = parseImport(template());
    expect(p.columns).toEqual([...COLUMNS]);
    expect(p.ignored).toEqual([]);
    expect(p.rows.map((r) => r.title)).toEqual(["Kaatrin Mozhi", "Hanuman Chalisa"]);
    expect(p.rows[0].singers).toEqual(["M. G. Sreekumar", "K. S. Chithra"]);
    expect(p.rows[1].occasions).toEqual(["Tuesday", "Hanuman Jayanti"]);
    expect(p.rows[0].lyrics).toContain("\n");
  });
});
