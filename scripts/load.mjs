// Locally reproducible load test. Seeds a catalogue through the public API, waits for the worker to
// index it, then drives four workloads and compares percentiles with the agreed SLOs.
// Run through scripts/load.sh (it starts the stack). Env: API, WEB, MAILBOX, DB, SONGS, SECS, CONC.
import { execFileSync } from "node:child_process";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const API = process.env.API, WEB = process.env.WEB, MAILBOX = process.env.MAILBOX, DB = process.env.DB;
const SONGS = +(process.env.SONGS ?? 300), SECS = +(process.env.SECS ?? 20), CONC = +(process.env.CONC ?? 16);
const PASSWORD = "correct horse battery staple";
const sql = (q) => execFileSync("psql", [DB, "-qAtc", q]).toString().trim();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** The agreed objectives. p95 in milliseconds, measured on a 5-core shared dev box (see docs). */
const SLO = { public_read: 150, search: 300, typeahead: 100, write: 400, web_page: 400 };
const MAX_ERROR_RATE = 0.005;
const ONLY = process.env.ONLY ? new Set(process.env.ONLY.split(",")) : null;
const want = (n) => !ONLY || ONLY.has(n);

class Client {
  jar = new Map();
  async req(method, path, body, base = API, extra = {}) {
    const headers = { "x-requested-with": "load", ...extra };
    if (body) headers["content-type"] = "application/json";
    if (this.jar.size) headers.cookie = [...this.jar].map(([k, v]) => `${k}=${v}`).join("; ");
    const res = await fetch(base + path, { method, headers, body: body ? JSON.stringify(body) : undefined, redirect: "manual" });
    for (const c of res.headers.getSetCookie()) { const [k, v] = c.split(";")[0].split("="); if (v) this.jar.set(k, v); }
    return res;
  }
  async json(method, path, body) {
    const res = await this.req(method, path, body);
    const text = await res.text();
    if (!res.ok) throw new Error(`${method} ${path} -> ${res.status} ${text.slice(0, 200)}`);
    return text ? JSON.parse(text) : null;
  }
}

async function signUp(email, role) {
  const c = new Client();
  await c.json("POST", "/api/v1/auth/register", { email, display_name: "Load", password: PASSWORD });
  let link = "";
  for (let i = 0; i < 100 && !link; i++) {
    for (const f of await readdir(MAILBOX).catch(() => [])) {
      const m = JSON.parse(await readFile(join(MAILBOX, f), "utf8"));
      if (m.topic === "email.verify_requested" && m.payload.to === email) link = m.payload.link;
    }
    if (!link) await sleep(100);
  }
  await c.json("POST", "/api/v1/auth/verify-email", { token: new URL(link).hash.split("token=")[1] });
  if (role) sql(`INSERT INTO user_roles (user_id, role) SELECT id, '${role}' FROM users WHERE email = '${email}'`);
  await c.json("POST", "/api/v1/auth/login", { email, password: PASSWORD });
  return c;
}

const percentile = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))];

/** Runs `fn(i)` from CONC loops for SECS seconds, timing each call. `fn` returns an HTTP status. */
async function drive(name, fn, conc = CONC) {
  const lat = []; let errors = 0, limited = 0, n = 0;
  const end = Date.now() + SECS * 1000;
  await Promise.all(Array.from({ length: conc }, async () => {
    while (Date.now() < end) {
      const t0 = performance.now();
      let status = 0;
      try { status = await fn(n++); } catch { status = 599; }
      const ms = performance.now() - t0;
      if (status === 429) limited++;           // an intended refusal, not a failure
      else if (status >= 400 || status === 0) errors++;
      else lat.push(ms);
    }
  }));
  lat.sort((a, b) => a - b);
  const ok = lat.length;
  return { name, requests: ok + errors + limited, ok, errors, limited, rps: +(ok / SECS).toFixed(1),
    p50: +percentile(lat, 50).toFixed(1), p95: +percentile(lat, 95).toFixed(1), p99: +percentile(lat, 99).toFixed(1), max: +lat[ok - 1].toFixed(1) };
}

const WORDS = ["river", "moon", "garden", "rain", "ocean", "lantern", "mountain", "harvest", "midnight", "desert"];
const stamp = Date.now().toString(36);

async function seed() {
  const mod = await signUp(`load-mod-${stamp}@example.com`, "moderator");
  const author = await signUp(`load-author-${stamp}@example.com`);
  const songs = [];
  let next = 0;
  await Promise.all(Array.from({ length: 8 }, async () => {
    while (next < SONGS) {
      const i = next++;
      const title = `Load ${stamp} ${i} ${WORDS[i % WORDS.length]}`;
      const w = await author.json("POST", "/api/v1/compositions", { title });
      const r = await author.json("POST", "/api/v1/recordings", { composition_id: w.id, title: "Take 1" });
      const body = Array.from({ length: 24 }, (_, l) => `line ${l} of ${title}`).join("\n");
      const c = await mod.json("POST", `/api/v1/recordings/${r.id}/contributions`, { type: "original", language: "ml", script: "Mlym", body });
      songs.push({ title, slug: title.toLowerCase().replace(/[^a-z0-9]+/g, "-"), recording: r.id, contribution: c.id });
    }
  }));
  return songs;
}

async function waitForIndex(songs) {
  const t0 = Date.now();
  for (;;) {
    const pending = +sql("SELECT count(*) FROM outbox WHERE processed_at IS NULL");
    if (pending === 0) return (Date.now() - t0) / 1000;
    if (Date.now() - t0 > 180000) throw new Error(`worker did not drain the outbox: ${pending} pending`);
    await sleep(500);
  }
}

const rnd = (a) => a[Math.floor(Math.random() * a.length)];

async function main() {
  const t = Date.now();
  const songs = await seed();
  console.log(`seeded ${songs.length} songs in ${((Date.now() - t) / 1000).toFixed(1)}s`);
  const drain = await waitForIndex(songs);
  console.log(`worker drained index/slug/SEO events in ${drain.toFixed(1)}s`);
  const anon = new Client();
  const get = async (path, base = API, h = {}) => { const r = await anon.req("GET", path, null, base, h); await r.arrayBuffer(); return r.status; };
  const ip = (i) => ({ "x-forwarded-for": `10.${(i >> 8) & 255}.${i & 255}.7` });   // many distinct clients, as behind the edge

  const writers = await Promise.all(Array.from({ length: 6 }, (_, i) => signUp(`load-w${i}-${stamp}@example.com`)));
  const results = [];
  let coldRender = null;
  if (want("web_page")) {
    // Warm pass: ISR renders each page once; the SLO is for what a reader gets afterwards. The cold cost is reported on its own.
    const lat = []; let next = 0;
    await Promise.all(Array.from({ length: 4 }, async () => {
      while (next < songs.length) { const t0 = performance.now(); await get(`/en/song/${songs[next++].slug}`, WEB); lat.push(performance.now() - t0); }
    }));
    lat.sort((a, b) => a - b);
    coldRender = { pages: lat.length, p50: +percentile(lat, 50).toFixed(1), p95: +percentile(lat, 95).toFixed(1) };
    console.log(`cold ISR render (4 at a time): p50 ${coldRender.p50}ms p95 ${coldRender.p95}ms over ${coldRender.pages} pages`);
  }
  if (want("public_read")) results.push(await drive("public_read", (i) => i % 2 ? get(`/api/v1/contributions/${rnd(songs).contribution}`) : get(`/api/v1/recordings/${rnd(songs).recording}/contributions`)));
  if (want("search")) results.push(await drive("search", () => get(`/api/v1/search?q=${rnd(WORDS)}&limit=20`, API)));
  if (want("typeahead")) results.push(await drive("typeahead", (i) => get(`/api/v1/search/suggest?q=${encodeURIComponent(rnd(WORDS).slice(0, 3))}`, API, ip(i % 600))));
  if (want("write")) results.push(await drive("write", async (i) => (await writers[i % writers.length].req("POST", "/api/v1/compositions", { title: `Written ${stamp} ${i} ${Math.random()}` })).status, 6));
  if (want("web_page")) results.push(await drive("web_page", () => get(`/en/song/${rnd(songs).slug}`, WEB)));

  console.table(results.map(({ name, ok, errors, limited, rps, p50, p95, p99, max }) => ({ name, ok, errors, limited, rps, p50, p95, p99, max })));
  const failures = [];
  for (const r of results) {
    const rate = r.errors / Math.max(1, r.requests);
    if (r.p95 > SLO[r.name]) failures.push(`${r.name}: p95 ${r.p95}ms > ${SLO[r.name]}ms`);
    if (rate > MAX_ERROR_RATE) failures.push(`${r.name}: error rate ${(rate * 100).toFixed(2)}%`);
  }
  await writeFile(process.env.OUT ?? "load-report.json", JSON.stringify({ songs: songs.length, secs: SECS, conc: CONC, slo_p95_ms: SLO, drain_secs: drain, cold_render: coldRender, results, failures }, null, 2));
  console.log(failures.length ? `SLO MISSED:\n  ${failures.join("\n  ")}` : "SLO MET for every workload");
  process.exit(failures.length && process.env.ENFORCE ? 1 : 0);
}
main().catch((e) => { console.error(e); process.exit(2); });
