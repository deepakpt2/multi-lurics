"use client";

import { useLocale, useTranslations } from "next-intl";
import { Link, usePathname } from "@/i18n/navigation";
import { locales, offeredLocales } from "@/i18n/routing";
import { SearchBox } from "./search/search";

export function SiteHeader() {
  const t = useTranslations();
  const current = useLocale();
  const pathname = usePathname();
  const NavLink = ({ href, children }: { href: string; children: React.ReactNode }) => (
    <Link href={href} aria-current={pathname === href || pathname.startsWith(`${href}/`) ? "page" : undefined}>
      {children}
    </Link>
  );
  return (
    <header className="site-header">
      <a className="skip-link" href="#main">
        {t("nav.skipToContent")}
      </a>
      <Link href="/" className="brand">
        {t("app.name")}
      </Link>
      <SearchBox />
      <nav aria-label={t("nav.primary")}>
        <NavLink href="/compositions">{t("nav.compositions")}</NavLink>
        <NavLink href="/recordings">{t("nav.recordings")}</NavLink>
        <NavLink href="/artists">{t("nav.artists")}</NavLink>
        <NavLink href="/albums">{t("nav.albums")}</NavLink>
        <NavLink href="/compare">{t("nav.compare")}</NavLink>
        <NavLink href="/moderation">{t("nav.moderation")}</NavLink>
        <NavLink href="/contributions">{t("nav.myLyrics")}</NavLink>
        <NavLink href="/account/comments">{t("nav.myComments")}</NavLink>
        <NavLink href="/login">{t("nav.signIn")}</NavLink>
        <NavLink href="/register">{t("nav.register")}</NavLink>
        <NavLink href="/account">{t("nav.account")}</NavLink>
      </nav>
      <nav aria-label={t("nav.language")} className="locale-switcher">
        {locales.map((l) =>
          offeredLocales.includes(l) ? (
            <Link key={l} href={pathname} locale={l} lang={l} aria-current={l === current ? "true" : undefined}>
              {t(`localeNames.${l}`)}
            </Link>
          ) : (
            <span key={l} lang={l} aria-disabled="true" className="disabled-locale">
              {t(`localeNames.${l}`)}
            </span>
          ),
        )}
      </nav>
    </header>
  );
}
