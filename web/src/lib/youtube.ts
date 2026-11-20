/**
 * YouTube videos are stored as their 11-character id, never as a link. This file is the only place that
 * knows what an embed address looks like, so if YouTube changes it, this is the one line to change.
 */
const ID = /^[A-Za-z0-9_-]{11}$/;

/** The id in whatever a person pasted (watch, youtu.be, shorts, embed, live, music links, or the bare id). */
export function youtubeId(input: string | null | undefined): string | null {
  const s = (input ?? "").trim();
  if (!s) return null;
  if (ID.test(s)) return s;
  let url: URL;
  try {
    url = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(s) ? s : `https://${s}`);
  } catch {
    return null;
  }
  const host = url.hostname.toLowerCase().replace(/^(www|m|music)\./, "");
  const parts = url.pathname.split("/").filter(Boolean);
  let id: string | undefined;
  if (host === "youtu.be") id = parts[0];
  else if (host === "youtube.com" || host === "youtube-nocookie.com") {
    if (parts[0] === "watch") id = url.searchParams.get("v") ?? undefined;
    else if (["embed", "shorts", "live", "v"].includes(parts[0] ?? "")) id = parts[1];
  }
  return id && ID.test(id) ? id : null;
}

/** Where the player for a video lives. Privacy-enhanced domain: no cookies until the viewer plays it. */
export const embedUrl = (id: string) => `https://www.youtube-nocookie.com/embed/${encodeURIComponent(id)}?rel=0`;

/** A normal link to the video, for "open on YouTube". */
export const watchUrl = (id: string) => `https://www.youtube.com/watch?v=${encodeURIComponent(id)}`;
