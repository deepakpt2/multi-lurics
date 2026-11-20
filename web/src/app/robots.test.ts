import { describe, expect, it } from "vitest";
import robots from "./robots";

describe("robots", () => {
  it("keeps private and id-based screens and query variants out, and points at the sitemap", () => {
    const r = robots();
    const rule = Array.isArray(r.rules) ? r.rules[0] : r.rules;
    const disallow = ([] as string[]).concat(rule.disallow ?? []);
    for (const p of ["/api/", "/*/account", "/*/moderation", "/*/compositions", "/*?*"]) expect(disallow).toContain(p);
    // The public routes are not blocked by any of those prefixes.
    expect(disallow.some((p) => "/en/song/x".startsWith(p.replace("/*", "/en")))).toBe(false);
    expect(r.sitemap).toBe("http://localhost:3000/sitemap.xml");
  });
});
