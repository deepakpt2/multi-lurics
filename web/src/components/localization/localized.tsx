"use client";

import { useLocale, useTranslations } from "next-intl";
import type { components } from "@/api/schema";

type Display = components["schemas"]["Display"];

/** Human name of a language tag in the reader's UI language (`ml` → "Malayalam"). */
export function languageName(tag: string, uiLocale: string): string {
  try {
    return new Intl.DisplayNames([uiLocale], { type: "language" }).of(tag) ?? tag;
  } catch {
    return tag;
  }
}

/**
 * Renders a server-resolved name. `lang` and `dir` come from the served text (not the UI locale),
 * so Malayalam inside an Arabic page keeps its own shaping and direction. Text that is not in the
 * reader's language is labelled, never silently substituted.
 */
export function Name({ display }: { display: Display }) {
  const t = useTranslations("localization");
  const locale = useLocale();
  const label = display.language ? t("shownIn", { language: languageName(display.language, locale) }) : t("originalName");
  return (
    <>
      <span lang={display.language ?? undefined} dir={display.dir} translate="no" data-resolution={display.resolution}>
        {display.text}
      </span>
      {display.fallback && (
        <small className="badge" data-fallback="true">
          {label}
        </small>
      )}
    </>
  );
}
