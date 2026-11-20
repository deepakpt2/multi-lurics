/**
 * Public pages on the live stack: Next (ISR) → Rust API → Postgres, with the worker creating
 * slugs and telling Next which cached pages to drop. Run via scripts/e2e.sh.
 */
import { execFileSync } from "node:child_process";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { call, createApi } from "@/api/client";

const BASE = process.env.E2E_BASE_URL!;
const MAILBOX = process.env.E2E_MAILBOX!;
const DB = process.env.E2E_DB_URL!;
const PASSWORD = "correct horse battery staple";

const sql = (q: string) => execFileSync("psql", [DB, "-qAtc", q]).toString().trim();

async function signUp(email: string, role?: "moderator") {
  const jar = new Map<string, string>();
  const f: typeof fetch = async (input, init) => {
    const req = new Request(input as Request, init);
    if (jar.size) req.headers.set("cookie", [...jar].map(([k, v]) => `${k}=${v}`).join("; "));
    const res = await fetch(req, { redirect: "manual" });
    for (const c of res.headers.getSetCookie()) {
      const [k, v] = c.split(";")[0].split("=");
      if (v) jar.set(k, v);
    }
    return res;
  };
  const api = createApi(BASE, f);
  await call(api.POST("/api/v1/auth/register", { body: { email, display_name: "Tester", password: PASSWORD } }));
  let link = "";
  for (let i = 0; i < 50 && !link; i++) {
    for (const file of await readdir(MAILBOX).catch(() => [])) {
      const mail = JSON.parse(await readFile(join(MAILBOX, file), "utf8"));
      if (mail.topic === "email.verify_requested" && mail.payload.to === email) link = mail.payload.link;
    }
    if (!link) await new Promise((r) => setTimeout(r, 200));
  }
  await call(api.POST("/api/v1/auth/verify-email", { body: { token: new URL(link).hash.split("token=")[1] } }));
  if (role) sql(`INSERT INTO user_roles (user_id, role) SELECT id, '${role}' FROM users WHERE email = '${email}'`);
  await call(api.POST("/api/v1/auth/login", { body: { email, password: PASSWORD } }));
  return api;
}

const get = (path: string) => fetch(`${BASE}${path}`, { redirect: "manual" });

/** Polls until the worker has indexed and Next has dropped its cached copy. */
async function until<T>(read: () => Promise<T>, ok: (v: T) => boolean, what: string): Promise<T> {
  let last: T | undefined;
  for (let i = 0; i < 60; i++) {
    last = await read();
    if (ok(last)) return last;
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`timed out waiting for ${what}`);
}

describe.runIf(BASE && MAILBOX && DB)("public pages (live stack)", () => {
  it("unpublished 404 → published page with metadata → rename redirects → unpublished again 404", async () => {
    const stamp = Date.now();
    const member = await signUp(`e2e-seo-m-${stamp}@example.com`);
    const moderator = await signUp(`e2e-seo-mod-${stamp}@example.com`, "moderator");
    const title = `Seo Song ${stamp}`;
    const slug = `seo-song-${stamp}`;
    const work = await call(member.POST("/api/v1/compositions", { body: { title } }));
    const recording = await call(member.POST("/api/v1/recordings", { body: { composition_id: work.id, title: "Take 1" } }));

    // Nothing is published: no page, and asking must not leave a cached 404 behind.
    expect((await get(`/en/song/${slug}`)).status).toBe(404);

    const lyric = await call(
      moderator.POST("/api/v1/recordings/{id}/contributions", {
        params: { path: { id: recording.id } },
        body: { type: "original", language: "ml", script: "Mlym", body: "ആദ്യ വരി\nരണ്ടാം വരി" },
      }),
    );
    expect(lyric.status).toBe("published");
    const live = await until(() => get(`/en/song/${slug}`), (r) => r.status === 200, "published page");
    const html = await live.text();
    expect(html).toContain(`<title>${title} lyrics</title>`);
    expect(html).toContain(`rel="canonical" href="${BASE}/en/song/${slug}"`);
    expect(html).toMatch(/hreflang="x-default"/i);
    expect(html).toContain('property="og:title"');
    expect(html).toContain('"@type":"MusicComposition"');
    expect(html).toContain("ആദ്യ വരി");
    expect(html).toMatch(/<html lang="en" dir="ltr"/);

    // Query strings do not make another page: the canonical stays the clean URL.
    const variant = await (await get(`/en/song/${slug}?utm_source=x&ref=y`)).text();
    expect(variant).toContain(`rel="canonical" href="${BASE}/en/song/${slug}"`);
    // A locale with no content for this song goes to the one that has it, permanently.
    const arabic = await get(`/ar/song/${slug}`);
    expect([301, 308]).toContain(arabic.status);
    expect(arabic.headers.get("location")).toContain(`/en/song/${slug}`);

    // Verified Arabic title: a second locale with content, in a non-Latin script.
    const arTitle = `أغنية ${stamp}`;
    sql(`INSERT INTO entity_localizations (entity_type, entity_id, language_id, script_id, name, is_preferred, provenance, source, verification_state, created_by, verified_by, verified_at)
         SELECT 'composition', '${work.id}', (SELECT id FROM languages WHERE iso639_3 = 'ara'), (SELECT id FROM scripts WHERE iso15924 = 'Arab'), '${arTitle}', true, 'original', 'e2e', 'verified', u.id, u.id, now()
         FROM users u LIMIT 1`);
    const arSlug = `أغنية-${stamp}`;
    const ar = await until(() => get(`/ar/song/${encodeURIComponent(arSlug)}`), (r) => r.status === 200, "arabic page");
    const arHtml = await ar.text();
    expect(arHtml).toMatch(/<html lang="ar" dir="rtl"/);
    expect(arHtml.toLowerCase()).toContain(`hreflang="en" href="${BASE}/en/song/${slug}"`);
    expect(arHtml.toLowerCase()).toContain(`hreflang="ar" href="${BASE}/ar/song/${encodeURIComponent(arSlug)}"`.toLowerCase());
    // The English page now lists both, after the worker invalidated its cached copy.
    const en = await until(async () => (await get(`/en/song/${slug}`)).text(), (t) => t.toLowerCase().includes('hreflang="ar"'), "english alternates");
    expect(en).toContain(`rel="canonical" href="${BASE}/en/song/${slug}"`);

    // Sitemap and robots.
    expect(await (await get("/robots.txt")).text()).toContain(`Sitemap: ${BASE}/sitemap.xml`);
    const index = await (await get("/sitemap.xml")).text();
    expect(index).toContain(`${BASE}/sitemaps/0.xml`);
    const map = await until(async () => (await get("/sitemaps/0.xml")).text(), (t) => t.includes(slug), "sitemap entry");
    expect(map).toContain(`<loc>${BASE}/en/song/${slug}</loc>`);
    expect(map).toContain('hreflang="x-default"');

    // Rename: the old address redirects to the new one for good.
    const renamed = `${title} Reprise`;
    await call(member.PATCH("/api/v1/compositions/{id}", { params: { path: { id: work.id } }, body: { title: renamed, expected_version: 1 } }));
    const moved = await until(() => get(`/en/song/${slug}`), (r) => [301, 308].includes(r.status), "redirect from the old slug");
    expect(moved.headers.get("location")).toContain(`/en/song/${slug}-reprise`);
    expect((await get(`/en/song/${slug}-reprise`)).status).toBe(200);

    // Withdrawn: gone for everyone, at once after invalidation.
    sql(`UPDATE contributions SET status = 'rejected', is_primary = false, published_revision_id = NULL WHERE recording_id = '${recording.id}'`);
    await until(() => get(`/en/song/${slug}-reprise`), (r) => r.status === 404, "unpublished page to disappear");
    expect((await get(`/en/song/${slug}`)).status).toBe(404);
  });

  it("the revalidate endpoint refuses anyone without the secret", async () => {
    const res = await fetch(`${BASE}/api/revalidate`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ tags: ["x"] }) });
    expect(res.status).toBe(401);
  });
});
