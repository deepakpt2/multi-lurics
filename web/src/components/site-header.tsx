"use client";

import { useLocale, useTranslations } from "next-intl";
import { Link, usePathname } from "@/i18n/navigation";
import { locales } from "@/i18n/routing";

export function SiteHeader() {
  const t = useTranslations();
  const current = useLocale();
  const pathname = usePathname();
  return (
    <header className="site-header">
      <a className="skip-link" href="#main">
        {t("nav.skipToContent")}
      </a>
      <Link href="/" className="brand">
        {t("app.name")}
      </Link>
      <nav aria-label={t("nav.primary")}>
        <Link href="/login">{t("nav.signIn")}</Link>
        <Link href="/register">{t("nav.register")}</Link>
        <Link href="/account">{t("nav.account")}</Link>
      </nav>
      <nav aria-label={t("nav.language")} className="locale-switcher">
        {locales.map((l) => (
          <Link key={l} href={pathname} locale={l} lang={l} aria-current={l === current ? "true" : undefined}>
            {t(`localeNames.${l}`)}
          </Link>
        ))}
      </nav>
    </header>
  );
}
