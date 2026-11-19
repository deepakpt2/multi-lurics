/**
 * Full stack: Next server (rewrites) → Rust API → Postgres, plus the outbox worker
 * delivering verification email to a dev mailbox. Run via scripts/e2e.sh.
 */
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
