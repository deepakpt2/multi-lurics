import { describe, expect, it, vi } from "vitest";
import { screen } from "@testing-library/react";
import { renderIntl } from "@/test/utils";
import { LocaleSwitcher } from "./locale-switcher";
import { SiteFooter } from "./site-footer";
import { endonyms, locales, offeredLocales } from "@/i18n/routing";

describe("language switcher", () => {
  it("lives in the footer and writes each language in itself", () => {
    renderIntl(<SiteFooter />);
    const footer = screen.getByRole("contentinfo");
    const nav = screen.getByRole("navigation", { name: "Language" });
    expect(footer).toContainElement(nav);
    expect(screen.getByRole("link", { name: "English" })).toHaveAttribute("lang", "en");
    expect(screen.getByRole("link", { name: "മലയാളം" })).toHaveAttribute("lang", "ml");
    // Arabic is built but not offered yet.
    expect(screen.queryByRole("link", { name: "العربية" })).not.toBeInTheDocument();
  });

  it("marks the current language", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    renderIntl(<LocaleSwitcher />);
    expect(screen.getByRole("link", { name: "English" })).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("link", { name: "മലയാളം" })).not.toHaveAttribute("aria-current");
  });

  it("every language has a name of its own", () => {
    for (const l of locales) expect(endonyms[l]).toBeTruthy();
    expect(offeredLocales).toContain("ml");
  });
});
