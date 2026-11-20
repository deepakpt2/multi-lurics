import { timingSafeEqual } from "node:crypto";
import { revalidateTag } from "next/cache";

const MAX_TAGS = 500;

function sameSecret(given: string | null, expected: string): boolean {
  const a = Buffer.from(given ?? "");
  const b = Buffer.from(expected);
  return a.length === b.length && timingSafeEqual(a, b);
}

/** Called by the worker (internal network) after content or a slug changed. */
export async function POST(req: Request) {
  const secret = process.env.REVALIDATE_SECRET;
  if (!secret) return Response.json({ error: "disabled" }, { status: 503 });
  if (!sameSecret(req.headers.get("x-revalidate-secret"), secret)) return Response.json({ error: "unauthorized" }, { status: 401 });
  const body = (await req.json().catch(() => null)) as { tags?: unknown } | null;
  const tags = body?.tags;
  if (!Array.isArray(tags) || tags.length > MAX_TAGS || !tags.every((t) => typeof t === "string" && t.length > 0 && t.length <= 300)) {
    return Response.json({ error: "invalid" }, { status: 400 });
  }
  for (const tag of new Set(tags as string[])) revalidateTag(tag);
  return Response.json({ revalidated: tags.length });
}
