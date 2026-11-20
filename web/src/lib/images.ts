import { ApiProblem, toProblem } from "@/api/client";
import type { components } from "@/api/schema";

export type Image = components["schemas"]["Image"];
export type ImageLimits = components["schemas"]["ImageLimits"];
/** Entity types that can have a picture. */
export const IMAGE_TYPES = ["artist", "album", "label", "publisher", "genre", "raga", "deity"] as const;
export type ImageEntity = (typeof IMAGE_TYPES)[number];

export const mediaUrl = (imageId: string, width: number) => `/api/v1/media/${imageId}/${width}`;

/** `src` and `srcSet` for a square image shown `size` CSS pixels wide (1x and 2x). */
export function mediaSources(imageId: string, size: number) {
  return { src: mediaUrl(imageId, size * 2), srcSet: `${mediaUrl(imageId, size)} 1x, ${mediaUrl(imageId, size * 2)} 2x` };
}

/** A request for a same-origin API path (an absolute URL, as the shared client builds them). */
const req = (path: string, init: RequestInit) => new Request(new URL(path, window.location.origin), { credentials: "same-origin", ...init });

/** Multipart upload; the shared API client cannot send a file, so this mirrors its error handling. */
export async function uploadImage(file: File, opts: { rights: boolean; credit?: string; entity?: { type: ImageEntity; id: string } }): Promise<Image> {
  const form = new FormData();
  form.set("file", file);
  form.set("rights", String(opts.rights));
  if (opts.credit?.trim()) form.set("credit", opts.credit.trim());
  if (opts.entity) {
    form.set("entity_type", opts.entity.type);
    form.set("entity_id", opts.entity.id);
  }
  let res: Response;
  try {
    res = await fetch(req("/api/v1/images", { method: "POST", body: form, headers: { "x-requested-with": "fetch" } }));
  } catch {
    throw toProblem(null, 0);
  }
  const body: unknown = await res.json().catch(() => null);
  if (!res.ok) throw toProblem(body, res.status);
  return body as Image;
}

export async function withdrawImage(id: string): Promise<void> {
  const res = await fetch(req(`/api/v1/images/${id}`, { method: "DELETE", headers: { "x-requested-with": "fetch" } }));
  if (!res.ok) throw toProblem(await res.json().catch(() => null), res.status);
}

let limits: Promise<ImageLimits> | undefined;
/** What an upload may be; fetched once per page load. */
export function loadLimits(): Promise<ImageLimits> {
  limits ??= fetch(req("/api/v1/images/limits", {}))
    .then((r) => (r.ok ? (r.json() as Promise<ImageLimits>) : Promise.reject(toProblem(null, r.status))))
    .catch((e: unknown) => {
      limits = undefined;
      throw e;
    });
  return limits;
}

export function formatBytes(n: number) {
  return n >= 1024 * 1024 ? `${Math.round((n / 1024 / 1024) * 10) / 10} MB` : `${Math.round(n / 1024)} KB`;
}

export { ApiProblem };
