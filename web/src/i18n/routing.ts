import { defineRouting } from "next-intl/routing";

export const locales = ["en", "ar"] as const;
export type Locale = (typeof locales)[number];

/** Locales shown in the language switcher. Arabic is built and kept, but switched off until it is ready: add "ar" here to re-enable. */
export const offeredLocales: readonly Locale[] = ["en"];

// localeDetection off: a browser's Accept-Language must not send visitors to a locale that is not offered.
export const routing = defineRouting({ locales, defaultLocale: "en", localeDetection: offeredLocales.length > 1 });

const RTL_LOCALES: ReadonlySet<string> = new Set(["ar", "he", "fa", "ur", "yi", "ps", "sd", "ug", "dv"]);

/** Layout direction for a UI locale. Driven by the locale's language, never by content. */
export function dirFor(locale: string): "rtl" | "ltr" {
  return RTL_LOCALES.has(locale.split("-")[0].toLowerCase()) ? "rtl" : "ltr";
}
