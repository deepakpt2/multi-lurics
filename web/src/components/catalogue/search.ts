import { api, call } from "@/api/client";
import type { PickOption } from "./common";

const LIMIT = 10;

export async function searchCompositions(prefix: string): Promise<PickOption[]> {
  const page = await call(api.GET("/api/v1/compositions", { params: { query: { prefix, limit: LIMIT } } }));
  return page.items.map((c) => ({ id: c.id, label: c.title, note: c.year != null ? String(c.year) : undefined }));
}

export async function searchRecordings(prefix: string): Promise<PickOption[]> {
  const page = await call(api.GET("/api/v1/recordings", { params: { query: { prefix, limit: LIMIT } } }));
  return page.items.map((r) => ({ id: r.id, label: r.title, note: r.isrc ?? undefined }));
}
