import { describe, expect, it } from "vitest";
import { ApiProblem, call, createApi } from "./client";
import { json, mockFetch, problem } from "@/test/utils";

describe("typed api client", () => {
  it("sends the CSRF header and returns typed data", async () => {
    const { calls } = mockFetch({
      "POST /api/v1/auth/login": () =>
        json(200, { id: "u1", email: "a@b.co", display_name: "A", roles: [], created_at: "2026-01-01T00:00:00Z" }),
    });
    const user = await call(createApi("http://localhost").POST("/api/v1/auth/login", { body: { email: "a@b.co", password: "x" } }));
    expect(user.display_name).toBe("A");
    expect(calls[0].headers.get("x-requested-with")).toBe("fetch");
  });

  it("turns problem+json into ApiProblem with field errors", async () => {
    mockFetch({
      "POST /api/v1/auth/register": () =>
        problem(422, "validation_failed", { errors: [{ field: "email", code: "invalid_email" }] }),
    });
    const err = await call(createApi("http://localhost").POST("/api/v1/auth/register", { body: {} as never })).catch((e) => e);
    expect(err).toBeInstanceOf(ApiProblem);
    expect(err.code).toBe("validation_failed");
    expect(err.fieldErrors).toEqual([{ field: "email", code: "invalid_email" }]);
  });

  it("exposes conflicts with the current revision id", async () => {
    mockFetch({ "GET /api/v1/me": () => problem(409, "stale_revision", { current_revision_id: "r2" }) });
    const err: ApiProblem = await call(createApi("http://localhost").GET("/api/v1/me")).catch((e) => e);
    expect(err.isConflict).toBe(true);
    expect(err.problem.current_revision_id).toBe("r2");
  });

  it("normalizes non-problem failures and network errors", async () => {
    mockFetch({ "GET /api/v1/me": () => new Response("<html>bad gateway</html>", { status: 502 }) });
    const bad: ApiProblem = await call(createApi("http://localhost").GET("/api/v1/me")).catch((e) => e);
    expect([bad.status, bad.code]).toEqual([502, "unexpected_response"]);

    mockFetch({ "GET /api/v1/me": () => { throw new TypeError("offline"); } });
    const down: ApiProblem = await call(createApi("http://localhost").GET("/api/v1/me")).catch((e) => e);
    expect([down.status, down.code]).toEqual([0, "network_error"]);
  });
});
