import { ApiProblem, toProblem } from "@/api/client";
import type { components } from "@/api/schema";
import { mediaUrl } from "./images";

export type Article = components["schemas"]["Article"];
export type ArticleImage = components["schemas"]["ArticleImage"];

/** Widths the server makes for article pictures. */
export const articleCover = (id: string) => ({ src: mediaUrl(id, 960), srcSet: `${mediaUrl(id, 480)} 480w, ${mediaUrl(id, 960)} 960w, ${mediaUrl(id, 1600)} 1600w` });

/** Value for `<input type="datetime-local">` from an ISO time ("" when none). */
export function toLocalInput(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** ISO time from a `datetime-local` value, or null when blank or not a date. */
export function fromLocalInput(value: string): string | null {
  if (!value.trim()) return null;
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

export type ParsedMarkdown = { title?: string; summary?: string; slug?: string; body: string };

/**
 * A Markdown file written elsewhere. Optional front matter (`title:`, `summary:`, `slug:` between `---`
 * lines) fills the fields; without it a leading `# Heading` becomes the title and is taken out of the text,
 * because the page shows the title itself.
 */
export function parseMarkdownFile(text: string): ParsedMarkdown {
  let body = text.replace(/^﻿/, "").replace(/\r\n?/g, "\n");
  const out: ParsedMarkdown = { body };
  const fm = /^---\n([\s\S]*?)\n---\n?/.exec(body);
  if (fm) {
    for (const line of fm[1].split("\n")) {
      const m = /^(title|summary|slug|description):\s*(.*)$/.exec(line.trim());
      if (!m) continue;
      const value = m[2].replace(/^(["'])(.*)\1$/, "$2").trim();
      if (!value) continue;
      if (m[1] === "title") out.title = value;
      else if (m[1] === "slug") out.slug = value;
      else out.summary = value;
    }
    body = body.slice(fm[0].length);
  }
  if (!out.title) {
    const h = /^\s*#[ \t]+(.+?)[ \t]*#*[ \t]*(?:\n|$)/.exec(body);
    if (h) {
      out.title = h[1];
      body = body.slice(h[0].length);
    }
  }
  out.body = body.replace(/^\n+/, "");
  return out;
}

/** Whether a picture address may be shown in an article: our own media or an https address. */
export const safeImageSrc = (src: string | undefined) => !!src && (src.startsWith("/api/v1/media/") || /^https:\/\//i.test(src));

/** Multipart upload of a picture for an article (the shared client cannot send files). */
export async function uploadArticleImage(articleId: string, file: File, credit?: string): Promise<ArticleImage> {
  const form = new FormData();
  form.set("file", file);
  if (credit?.trim()) form.set("credit", credit.trim());
  let res: Response;
  try {
    res = await fetch(new Request(new URL(`/api/v1/articles/${articleId}/images`, window.location.origin), { method: "POST", body: form, credentials: "same-origin", headers: { "x-requested-with": "fetch" } }));
  } catch {
    throw toProblem(null, 0);
  }
  const body: unknown = await res.json().catch(() => null);
  if (!res.ok) throw toProblem(body, res.status);
  return body as ArticleImage;
}

export const isProblem = (e: unknown): e is ApiProblem => e instanceof ApiProblem;
