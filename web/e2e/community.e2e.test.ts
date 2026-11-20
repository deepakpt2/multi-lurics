/**
 * Comparison and community on the live stack: Next proxy → Rust API → Postgres. Run via scripts/e2e.sh.
 */
import { execFileSync } from "node:child_process";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { ApiProblem, call, createApi } from "@/api/client";

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

describe.runIf(BASE && MAILBOX && DB)("comparison and community (live stack)", () => {
  it("resolves what is published, refuses self-rating, and keeps moderation behind the role", async () => {
    const stamp = Date.now();
    const author = await signUp(`e2e-cm-a-${stamp}@example.com`);
    const reader = await signUp(`e2e-cm-r-${stamp}@example.com`);
    const moderator = await signUp(`e2e-cm-m-${stamp}@example.com`, "moderator");
    const work = await call(author.POST("/api/v1/compositions", { body: { title: `Compare ${stamp}` } }));
    const recording = await call(author.POST("/api/v1/recordings", { body: { composition_id: work.id, title: "Take 1" } }));
    const lyric = await call(
      author.POST("/api/v1/recordings/{id}/contributions", {
        params: { path: { id: recording.id } },
        body: { type: "original", language: "ml", script: "Mlym", body: "ആദ്യ വരി" },
      }),
    );
    const resolveIds = (ids: string[]) => call(reader.GET("/api/v1/comparisons/resolve", { params: { query: { ids: ids.join(",") } } }));

    // Pending: no text, whoever asks.
    expect((await resolveIds([lyric.id])).panels[0]).toMatchObject({ id: lyric.id, available: false, content: null });

    await call(
      moderator.POST("/api/v1/contributions/{id}/publish", {
        params: { path: { id: lyric.id } },
        body: { expected_parent_revision_id: lyric.current_revision_id },
      }),
    );
    const live = (await resolveIds([lyric.id])).panels[0];
    expect(live.available).toBe(true);
    expect(live.content).toMatchObject({ body: "ആദ്യ വരി", language: "ml", script: "Mlym", is_primary: true });

    // The compare page itself renders from an address that holds ids only.
    const page = await fetch(`${BASE}/en/compare?c=${lyric.id}`);
    expect(page.status).toBe(200);
    expect(await page.text()).not.toContain("ആദ്യ വരി");

    // Community: the author cannot rate; a reader can; a member cannot moderate.
    const own = await author.PUT("/api/v1/contributions/{id}/rating", { params: { path: { id: lyric.id } }, body: { value: 5 } });
    expect(own.response.status).toBe(422);
    await call(reader.PUT("/api/v1/contributions/{id}/rating", { params: { path: { id: lyric.id } }, body: { value: 5 } }));
    expect((await call(reader.GET("/api/v1/contributions/{id}/engagement", { params: { path: { id: lyric.id } } }))).rating_average).toBe(5);
    await expect(call(reader.GET("/api/v1/moderation/reports"))).rejects.toMatchObject({ status: 403 } satisfies Partial<ApiProblem>);
    expect(sql(`SELECT count(*) FROM audit_log WHERE action = 'moderation.denied'`)).not.toBe("0");

    // Withdrawn: the comparison stops showing it at once.
    sql(`UPDATE contributions SET status = 'rejected', is_primary = false, published_revision_id = NULL WHERE id = '${lyric.id}'`);
    expect((await resolveIds([lyric.id])).panels[0].available).toBe(false);
  });
});
