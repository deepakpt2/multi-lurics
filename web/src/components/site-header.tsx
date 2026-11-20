"use client";

import * as Dropdown from "@radix-ui/react-dropdown-menu";
import { Disc3, Flower2, Library, ListMusic, LogIn, Menu, MessageSquare, Mic2, NotebookPen, ShieldCheck, Columns2, UserPlus, UserRound } from "lucide-react";
import { useLocale, useTranslations } from "next-intl";
import { Link, usePathname } from "@/i18n/navigation";
import { locales, offeredLocales } from "@/i18n/routing";
import { SearchBox } from "./search/search";

type Icon = typeof Library;

export function SiteHeader() {
  const t = useTranslations();
  const current = useLocale();
  const pathname = usePathname();
  const active = (href: string) => (pathname === href || pathname.startsWith(`${href}/`) ? "page" : undefined);
  const NavLink = ({ href, icon: I, children }: { href: string; icon: Icon; children: React.ReactNode }) => (
    <Link href={href} aria-current={active(href)}>
      <I size={16} aria-hidden="true" />
      {children}
    </Link>
  );
  const MenuLink = ({ href, icon: I, children }: { href: string; icon: Icon; children: React.ReactNode }) => (
    <Dropdown.Item asChild>
      <Link href={href} className="menu-item">
        <I size={16} aria-hidden="true" />
        {children}
      </Link>
    </Dropdown.Item>
  );
  return (
    <header className="site-header">
      <a className="skip-link" href="#main">
        {t("nav.skipToContent")}
      </a>
      <Link href="/" className="brand">
        <Flower2 size={22} aria-hidden="true" />
        {t("app.name")}
      </Link>
      <SearchBox />
      <nav aria-label={t("nav.primary")}>
        <NavLink href="/compositions" icon={Library}>{t("nav.compositions")}</NavLink>
        <NavLink href="/recordings" icon={Disc3}>{t("nav.recordings")}</NavLink>
        <NavLink href="/artists" icon={Mic2}>{t("nav.artists")}</NavLink>
        <NavLink href="/albums" icon={ListMusic}>{t("nav.albums")}</NavLink>
        <NavLink href="/compare" icon={Columns2}>{t("nav.compare")}</NavLink>
      </nav>
      <Dropdown.Root>
        <Dropdown.Trigger className="menu-trigger">
          <Menu size={18} aria-hidden="true" />
          <span>{t("nav.menu")}</span>
        </Dropdown.Trigger>
        <Dropdown.Portal>
          <Dropdown.Content className="menu" align="end" sideOffset={8}>
            <MenuLink href="/account" icon={UserRound}>{t("nav.account")}</MenuLink>
            <MenuLink href="/contributions" icon={NotebookPen}>{t("nav.myLyrics")}</MenuLink>
            <MenuLink href="/account/comments" icon={MessageSquare}>{t("nav.myComments")}</MenuLink>
            <MenuLink href="/moderation" icon={ShieldCheck}>{t("nav.moderation")}</MenuLink>
            <Dropdown.Separator className="menu-sep" />
            <MenuLink href="/login" icon={LogIn}>{t("nav.signIn")}</MenuLink>
            <MenuLink href="/register" icon={UserPlus}>{t("nav.register")}</MenuLink>
          </Dropdown.Content>
        </Dropdown.Portal>
      </Dropdown.Root>
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
