import { describe, expect, it } from "vitest";
import en from "../../messages/en.json";
import ar from "../../messages/ar.json";
import { dirFor, locales } from "./routing";

function keys(obj: object, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([k, v]) =>
    v && typeof v === "object" ? keys(v, `${prefix}${k}.`) : [`${prefix}${k}`],
  );
}

describe("message catalogs", () => {
  it("every locale has exactly the same keys as English", () => {
    expect(keys(ar).sort()).toEqual(keys(en).sort());
  });

  it("has no empty strings", () => {
    for (const cat of [en, ar]) {
      const flat = keys(cat).map((k) => k.split(".").reduce<unknown>((o, p) => (o as Record<string, unknown>)[p], cat));
      expect(flat.every((v) => typeof v === "string" && v.trim().length > 0)).toBe(true);
    }
  });

  it("covers every server error and validation code", async () => {
    // Codes the API can emit (crates/api/src/problem.rs, crates/domain); keep in sync.
    const errorCodes = [
      "validation_failed", "unauthenticated", "invalid_credentials", "email_not_verified", "forbidden",
      "invalid_token", "not_found", "unavailable", "internal", "csrf_header_missing", "malformed_json",
      "invalid_body", "unsupported_media_type", "invalid_cursor", "invalid_query",
    ];
    for (const c of errorCodes) expect(en.errors).toHaveProperty(c);
    for (const c of ["required", "invalid_email", "too_short", "too_long", "invalid_characters", "unsupported_locale", "out_of_range"])
      expect(en.validation).toHaveProperty(c);
  });
});

describe("layout direction", () => {
  it("derives direction from the UI locale", () => {
    expect(dirFor("ar")).toBe("rtl");
    expect(dirFor("ar-EG")).toBe("rtl");
    expect(dirFor("he")).toBe("rtl");
    expect(dirFor("en")).toBe("ltr");
    expect(dirFor("ta")).toBe("ltr");
  });
  it("every supported locale has a name for the switcher", () => {
    for (const l of locales) expect(en.localeNames).toHaveProperty(l);
  });
});
