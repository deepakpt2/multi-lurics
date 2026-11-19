/**
 * Full stack: Next server (rewrites) → Rust API → Postgres, plus the outbox worker
 * delivering verification email to a dev mailbox. Run via scripts/e2e.sh.
 */
import { execFileSync } from "node:child_process";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { ApiProblem, call, createApi } from "@/api/client";

const BASE = process.env.E2E_BASE_URL!;
const MAILBOX = process.env.E2E_MAILBOX!;

/** Minimal cookie jar so the Node client behaves like a browser session. */
function browserLikeFetch() {
  const jar = new Map<string, string>();
  const f: typeof fetch = async (input, init) => {
    const req = new Request(input as Request, init);
    if (jar.size) req.headers.set("cookie", [...jar].map(([k, v]) => `${k}=${v}`).join("; "));
    const res = await fetch(req, { redirect: "manual" });
    for (const c of res.headers.getSetCookie()) {
      const [pair, ...attrs] = c.split(";");
      const [k, v] = pair.split("=");
      if (attrs.some((a) => /max-age=0/i.test(a.trim())) || !v) jar.delete(k);
      else jar.set(k, v);
    }
    return res;
  };
  return { fetch: f, jar };
}

async function waitForEmail(to: string, topic: string): Promise<string> {
  for (let i = 0; i < 50; i++) {
    for (const f of await readdir(MAILBOX).catch(() => [])) {
      const mail = JSON.parse(await readFile(join(MAILBOX, f), "utf8"));
      if (mail.topic === topic && mail.payload.to === to) return mail.payload.link;
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`no ${topic} email for ${to}`);
}

const problemOf = (p: Promise<unknown>) => p.then(() => null, (e: ApiProblem) => e);

describe.runIf(BASE && MAILBOX)("frontend → protected endpoint (live stack)", () => {
  it("register → verify → login → /me, with deny cases", async () => {
    const browser = browserLikeFetch();
    const api = createApi(BASE, browser.fetch);
    const email = `e2e-${Date.now()}@example.com`;

    const anon = await problemOf(call(api.GET("/api/v1/me")));
    expect([anon?.status, anon?.code]).toEqual([401, "unauthenticated"]);

    await call(api.POST("/api/v1/auth/register", { body: { email, display_name: "فيروز", password: "correct horse battery staple", locale: "ar" } }));
    const link = await waitForEmail(email, "email.verify_requested");
    expect(link).toMatch(/\/ar\/verify-email#token=/);
    const token = new URL(link).hash.split("token=")[1];

    const early = await problemOf(call(api.POST("/api/v1/auth/login", { body: { email, password: "correct horse battery staple" } })));
    expect(early?.code).toBe("email_not_verified");

    await call(api.POST("/api/v1/auth/verify-email", { body: { token } }));
    await call(api.POST("/api/v1/auth/login", { body: { email, password: "correct horse battery staple" } }));
    expect(browser.jar.has("lyrics_session")).toBe(true);

    const me = await call(api.GET("/api/v1/me"));
    expect(me.email).toBe(email);
    expect(me.display_name).toBe("فيروز");

    const denied = await problemOf(call(api.GET("/api/v1/admin/audit-events", { params: { query: {} } })));
    expect([denied?.status, denied?.code]).toEqual([403, "forbidden"]);

    await call(api.POST("/api/v1/auth/logout"));
    const after = await problemOf(call(api.GET("/api/v1/me")));
    expect(after?.status).toBe(401);
  });

  it("catalogue round trip through the Next proxy: create, cover, stale PATCH, anonymous deny", async () => {
    const browser = browserLikeFetch();
    const api = createApi(BASE, browser.fetch);
    const email = `e2e-cat-${Date.now()}@example.com`;
    const password = "correct horse battery staple";
    await call(api.POST("/api/v1/auth/register", { body: { email, display_name: "Editor", password } }));
    const token = new URL(await waitForEmail(email, "email.verify_requested")).hash.split("token=")[1];
    await call(api.POST("/api/v1/auth/verify-email", { body: { token } }));
    await call(api.POST("/api/v1/auth/login", { body: { email, password } }));

    const singer = await call(api.POST("/api/v1/artists", { body: { name: "இளையராஜா", kind: "person" } }));
    const work = await call(
      api.POST("/api/v1/compositions", { body: { title: "Work", credits: [{ artist_id: singer.id, role: "composer" }, { artist_id: singer.id, role: "lyricist" }] } }),
    );
    const original = await call(api.POST("/api/v1/recordings", { body: { composition_id: work.id, title: "Original" } }));
    const cover = await call(
      api.POST("/api/v1/recordings", {
        body: { composition_id: work.id, title: "Cover", relationships: [{ kind: "cover", related_recording_id: original.id }] },
      }),
    );
    expect(cover.composition.id).toBe(work.id);
    expect(cover.credits).toEqual([]);

    const renamed = await call(
      api.PATCH("/api/v1/artists/{id}", { params: { path: { id: singer.id } }, body: { expected_version: 1, disambiguation: "composer" } }),
    );
    expect(renamed.version).toBe(2);
    const stale = await problemOf(
      call(api.PATCH("/api/v1/artists/{id}", { params: { path: { id: singer.id } }, body: { expected_version: 1, name: "X" } })),
    );
    expect([stale?.status, stale?.code, stale?.problem.current_version]).toEqual([409, "version_conflict", 2]);

    const detail = await call(api.GET("/api/v1/compositions/{id}", { params: { path: { id: work.id } } }));
    expect(detail.recording_count).toBe(2);

    const anon = createApi(BASE);
    const denied = await problemOf(call(anon.POST("/api/v1/artists", { body: { name: "N", kind: "person" } })));
    expect([denied?.status, denied?.code]).toEqual([401, "unauthenticated"]);

    const page = await (await fetch(`${BASE}/ar/compositions/${work.id}`)).text();
    expect(page).toMatch(/<html lang="ar" dir="rtl"/);
  });

  it("localized names through the Next proxy: submit, moderate, then resolve with fallback labels", async () => {
    const password = "correct horse battery staple";
    async function signUp(email: string) {
      const browser = browserLikeFetch();
      const api = createApi(BASE, browser.fetch);
      await call(api.POST("/api/v1/auth/register", { body: { email, display_name: "Tester", password } }));
      const token = new URL(await waitForEmail(email, "email.verify_requested")).hash.split("token=")[1];
      await call(api.POST("/api/v1/auth/verify-email", { body: { token } }));
      await call(api.POST("/api/v1/auth/login", { body: { email, password } }));
      return api;
    }
    const stamp = Date.now();
    const member = await signUp(`e2e-loc-m-${stamp}@example.com`);
    const modEmail = `e2e-loc-mod-${stamp}@example.com`;
    const moderator = await signUp(modEmail);
    execFileSync("psql", [process.env.E2E_DB_URL!, "-qc", `INSERT INTO user_roles (user_id, role) SELECT id, 'moderator' FROM users WHERE email = '${modEmail}'`]);

    const work = await call(member.POST("/api/v1/compositions", { body: { title: "Kaatrin Mozhi" } }));
    const path = { entity_type: "composition", entity_id: work.id };
    const submitted = await call(
      member.POST("/api/v1/entities/{entity_type}/{entity_id}/localizations", {
        params: { path },
        body: { language: "ml", script: "Mlym", name: "കാറ്റിൻ മൊഴി", provenance: "community" },
      }),
    );
    expect(submitted.verification_state).toBe("pending");

    const anon = createApi(BASE);
    const read = (lang: string) => call(anon.GET("/api/v1/compositions/{id}", { params: { path: { id: work.id }, query: { lang } } }));
    const before = await read("ml");
    expect([before.display.text, before.display.resolution, before.display.fallback]).toEqual(["Kaatrin Mozhi", "base", true]);

    const queue = await call(moderator.GET("/api/v1/moderation/localizations", { params: { query: {} } }));
    expect(queue.items.map((i) => i.id)).toContain(submitted.id);
    const denied = await problemOf(call(member.GET("/api/v1/moderation/localizations", { params: { query: {} } })));
    expect([denied?.status, denied?.code]).toEqual([403, "forbidden"]);
    const stale = await problemOf(
      call(moderator.POST("/api/v1/localizations/{id}/verify", { params: { path: { id: submitted.id } }, body: { expected_parent_revision_id: "00000000-0000-4000-8000-000000000000" } })),
    );
    expect([stale?.status, stale?.code, stale?.problem.current_revision_id]).toEqual([409, "stale_revision", submitted.current_revision_id]);
    await call(
      moderator.POST("/api/v1/localizations/{id}/verify", { params: { path: { id: submitted.id } }, body: { expected_parent_revision_id: submitted.current_revision_id } }),
    );

    const ml = await read("ml");
    expect([ml.display.text, ml.display.language, ml.display.dir, ml.display.fallback]).toEqual(["കാറ്റിൻ മൊഴി", "ml-Mlym", "ltr", false]);
    const ar = await read("ar");
    expect([ar.display.text, ar.display.resolution, ar.display.fallback]).toEqual(["കാറ്റിൻ മൊഴി", "fallback", true]);

    const html = await (await fetch(`${BASE}/ar/names/composition/${work.id}`)).text();
    expect(html).toMatch(/<html lang="ar" dir="rtl"/);
  });

  it("lyrics: submit → invisible to the public → moderator publishes → public reads it through the proxy", async () => {
    const password = "correct horse battery staple";
    async function signUp(email: string) {
      const browser = browserLikeFetch();
      const api = createApi(BASE, browser.fetch);
      await call(api.POST("/api/v1/auth/register", { body: { email, display_name: "Tester", password } }));
      const token = new URL(await waitForEmail(email, "email.verify_requested")).hash.split("token=")[1];
      await call(api.POST("/api/v1/auth/verify-email", { body: { token } }));
      await call(api.POST("/api/v1/auth/login", { body: { email, password } }));
      return api;
    }
    const stamp = Date.now();
    const member = await signUp(`e2e-lyr-m-${stamp}@example.com`);
    const modEmail = `e2e-lyr-mod-${stamp}@example.com`;
    const moderator = await signUp(modEmail);
    execFileSync("psql", [process.env.E2E_DB_URL!, "-qc", `INSERT INTO user_roles (user_id, role) SELECT id, 'moderator' FROM users WHERE email = '${modEmail}'`]);

    const work = await call(member.POST("/api/v1/compositions", { body: { title: "Kaatrin Mozhi" } }));
    const rec = await call(member.POST("/api/v1/recordings", { body: { composition_id: work.id, title: "Take 1" } }));
    const key = `e2e-key-${stamp}`;
    const make = () =>
      member.POST("/api/v1/recordings/{id}/contributions", {
        params: { path: { id: rec.id }, header: { "Idempotency-Key": key } },
        body: { type: "translation", language: "ar", script: "Arab", body: "كلمات\nالأغنية" },
      });
    const created = await call(make());
    expect(created.status).toBe("pending");
    expect((await call(make())).id).toBe(created.id); // a retry returns the first result

    // The browser's `lang` parameter is accepted on these reads, and nothing unpublished is served.
    const anon = createApi(BASE);
    const hidden = await problemOf(call(anon.GET("/api/v1/contributions/{id}", { params: { path: { id: created.id } } })));
    expect([hidden?.status, hidden?.code]).toEqual([404, "not_found"]);
    const list = await call(anon.GET("/api/v1/recordings/{id}/contributions", { params: { path: { id: rec.id }, query: {} } }));
    expect(list.items).toHaveLength(0);

    const stale = await problemOf(
      call(moderator.POST("/api/v1/contributions/{id}/publish", { params: { path: { id: created.id } }, body: { expected_parent_revision_id: "00000000-0000-4000-8000-000000000000" } })),
    );
    expect([stale?.status, stale?.code, stale?.problem.current_revision_id]).toEqual([409, "stale_revision", created.current_revision_id]);
    const denied = await problemOf(
      call(member.POST("/api/v1/contributions/{id}/publish", { params: { path: { id: created.id } }, body: { expected_parent_revision_id: created.current_revision_id } })),
    );
    expect([denied?.status, denied?.code]).toEqual([403, "forbidden"]);
    await call(moderator.POST("/api/v1/contributions/{id}/publish", { params: { path: { id: created.id } }, body: { expected_parent_revision_id: created.current_revision_id } }));

    const pub = await call(anon.GET("/api/v1/contributions/{id}", { params: { path: { id: created.id } } }));
    expect([pub.published?.body, pub.dir, pub.is_primary, pub.head]).toEqual(["كلمات\nالأغنية", "rtl", true, null]);
    const res = await fetch(`${BASE}/api/v1/contributions/${created.id}`);
    expect(res.headers.get("cache-control")).toBe("private, no-store");
    const html = await (await fetch(`${BASE}/ar/contributions/${created.id}`)).text();
    expect(html).toMatch(/<html lang="ar" dir="rtl"/);
  });

  it("serves localized, direction-correct pages", async () => {
    const ar = await (await fetch(`${BASE}/ar/account`)).text();
    expect(ar).toMatch(/<html lang="ar" dir="rtl"/);
    const en = await (await fetch(`${BASE}/en/login`)).text();
    expect(en).toMatch(/<html lang="en" dir="ltr"/);
    const root = await fetch(`${BASE}/`, { redirect: "manual" });
    expect(root.status).toBeGreaterThanOrEqual(300);
    expect(root.headers.get("location")).toMatch(/\/en$/);
  });
});
