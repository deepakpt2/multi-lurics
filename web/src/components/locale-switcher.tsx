"use client";

import { useLocale, useTranslations } from "next-intl";
import { Link, usePathname } from "@/i18n/navigation";
import { endonyms, locales, offeredLocales } from "@/i18n/routing";

/** The site's language, chosen from the footer. Each language is written in itself. */
export function LocaleSwitcher() {
  const t = useTranslations("nav");
  const current = useLocale();
  const pathname = usePathname();
  const offered = locales.filter((l) => offeredLocales.includes(l));
  if (offered.length < 2) return null;
  return (
    <nav aria-label={t("language")} className="locale-switcher">
      <ul>
        {offered.map((l) => (
          <li key={l}>
            <Link href={pathname} locale={l} lang={l} hrefLang={l} aria-current={l === current ? "true" : undefined}>
              {endonyms[l]}
            </Link>
          </li>
        ))}
      </ul>
    </nav>
  );
}
