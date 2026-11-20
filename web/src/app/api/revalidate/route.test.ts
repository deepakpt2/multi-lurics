// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const revalidateTag = vi.fn();
vi.mock("next/cache", () => ({ revalidateTag: (t: string) => revalidateTag(t) }));
const { POST } = await import("./route");

const call = (body: unknown, secret?: string) =>
  POST(new Request("http://x/api/revalidate", { method: "POST", headers: { "content-type": "application/json", ...(secret ? { "x-revalidate-secret": secret } : {}) }, body: typeof body === "string" ? body : JSON.stringify(body) }));

describe("revalidate endpoint", () => {
  beforeEach(() => revalidateTag.mockReset());
  afterEach(() => vi.unstubAllEnvs());

  it("is off until a secret is configured", async () => {
    vi.stubEnv("REVALIDATE_SECRET", "");
    expect((await call({ tags: ["a"] }, "anything")).status).toBe(503);
  });

  it("rejects a missing or wrong secret without touching the cache", async () => {
    vi.stubEnv("REVALIDATE_SECRET", "right-secret-right-secret");
    expect((await call({ tags: ["a"] })).status).toBe(401);
    expect((await call({ tags: ["a"] }, "wrong-secret-wrong-secret")).status).toBe(401);
    expect((await call({ tags: ["a"] }, "right")).status).toBe(401);
    expect(revalidateTag).not.toHaveBeenCalled();
  });

  it("validates the body", async () => {
    vi.stubEnv("REVALIDATE_SECRET", "right-secret-right-secret");
    for (const bad of ["not json", {}, { tags: "a" }, { tags: [1] }, { tags: [""] }, { tags: ["x".repeat(301)] }, { tags: Array.from({ length: 501 }, (_, i) => `t${i}`) }]) {
      expect((await call(bad, "right-secret-right-secret")).status, JSON.stringify(bad).slice(0, 40)).toBe(400);
    }
    expect(revalidateTag).not.toHaveBeenCalled();
  });

  it("drops each distinct tag once", async () => {
    vi.stubEnv("REVALIDATE_SECRET", "right-secret-right-secret");
    const res = await call({ tags: ["slug:song:en:a", "slug:song:en:a", "slug:song:ar:كاترين"] }, "right-secret-right-secret");
    expect(res.status).toBe(200);
    expect(revalidateTag.mock.calls.map((c) => c[0])).toEqual(["slug:song:en:a", "slug:song:ar:كاترين"]);
  });
});
