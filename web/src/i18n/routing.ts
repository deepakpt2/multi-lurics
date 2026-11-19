import { defineRouting } from "next-intl/routing";

export const locales = ["en", "ar"] as const;
export type Locale = (typeof locales)[number];

export const routing = defineRouting({ locales, defaultLocale: "en" });

const RTL_LOCALES: ReadonlySet<string> = new Set(["ar", "he", "fa", "ur", "yi", "ps", "sd", "ug", "dv"]);

/** Layout direction for a UI locale. Driven by the locale's language, never by content. */
export function dirFor(locale: string): "rtl" | "ltr" {
  return RTL_LOCALES.has(locale.split("-")[0].toLowerCase()) ? "rtl" : "ltr";
}
