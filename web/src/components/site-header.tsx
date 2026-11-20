"use client";

import * as Dropdown from "@radix-ui/react-dropdown-menu";
import { Columns2, Disc3, FileUp, Flower2, PenLine, Library, ListMusic, LogIn, LogOut, MessageSquare, Mic2, NotebookPen, ShieldCheck, UserPlus, UserRound, Users } from "lucide-react";
import { Menu as MenuIcon, X as CloseIcon } from "lucide-react";
import { useTranslations } from "next-intl";
import { useEffect, useState } from "react";
import { api, call } from "@/api/client";
import { Link, usePathname, useRouter } from "@/i18n/navigation";
import { SearchBox } from "./search/search";
import { useSession } from "./session";

type Icon = typeof Library;

export function SiteHeader() {
  const t = useTranslations();
  const pathname = usePathname();
  const router = useRouter();
  const [open, setOpen] = useState(false);
  // Choosing something closes the phone menu.
  useEffect(() => setOpen(false), [pathname]);
  const { user, isModerator, isAdmin } = useSession();
  const signOut = async () => {
    try {
      await call(api.POST("/api/v1/auth/logout"));
    } finally {
      router.push("/login");
    }
  };
  const active = (href: string, exact = false) =>
    pathname === href || (!exact && pathname.startsWith(`${href}/`)) ? "page" : undefined;
  const NavLink = ({ href, icon: I, exact, children }: { href: string; icon: Icon; exact?: boolean; children: React.ReactNode }) => (
    <Link href={href} aria-current={active(href, exact)}>
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
      <button type="button" className="burger" aria-expanded={open} aria-controls="header-menu" aria-label={t("nav.siteNav")} onClick={() => setOpen((o) => !o)}>
        {open ? <CloseIcon size={20} aria-hidden="true" /> : <MenuIcon size={20} aria-hidden="true" />}
      </button>
      <div id="header-menu" className="header-menu" data-open={open || undefined}>
      <Link href="/contribute" className="button primary header-contribute">
        <PenLine size={16} aria-hidden="true" />
        {t("nav.contribute")}
      </Link>
      <nav aria-label={t("nav.primary")}>
        <NavLink href="/browse" icon={Library} exact>{t("nav.browse")}</NavLink>
        <NavLink href="/browse/album" icon={ListMusic}>{t("nav.albums")}</NavLink>
        <NavLink href="/browse/artist" icon={Mic2}>{t("nav.artists")}</NavLink>
        <NavLink href="/compare" icon={Columns2}>{t("nav.compare")}</NavLink>
      </nav>
      {user === null && (
        <div className="auth-links">
          <Link href="/login" className="button">
            <LogIn size={16} aria-hidden="true" />
            {t("nav.signIn")}
          </Link>
          <Link href="/register" className="button primary">
            <UserPlus size={16} aria-hidden="true" />
            {t("nav.register")}
          </Link>
        </div>
      )}
      {user && (
        <Dropdown.Root>
          <Dropdown.Trigger className="menu-trigger" aria-label={t("nav.menu")}>
            <UserRound size={18} aria-hidden="true" />
            <span dir="auto" translate="no">{user.display_name}</span>
          </Dropdown.Trigger>
          <Dropdown.Portal>
            <Dropdown.Content className="menu" align="end" sideOffset={8}>
              <MenuLink href="/account" icon={UserRound}>{t("nav.account")}</MenuLink>
              <MenuLink href="/contributions" icon={NotebookPen}>{t("nav.myLyrics")}</MenuLink>
              <MenuLink href="/account/comments" icon={MessageSquare}>{t("nav.myComments")}</MenuLink>
              <Dropdown.Separator className="menu-sep" />
              <Dropdown.Label className="menu-label">{t("nav.catalogueTools")}</Dropdown.Label>
              <MenuLink href="/compositions" icon={Library}>{t("nav.compositions")}</MenuLink>
              <MenuLink href="/recordings" icon={Disc3}>{t("nav.recordings")}</MenuLink>
              <MenuLink href="/artists" icon={Mic2}>{t("nav.artists")}</MenuLink>
              <MenuLink href="/albums" icon={ListMusic}>{t("nav.albums")}</MenuLink>
              {(isModerator || isAdmin) && <Dropdown.Separator className="menu-sep" />}
              {isModerator && <MenuLink href="/moderation" icon={ShieldCheck}>{t("nav.moderation")}</MenuLink>}
              {isModerator && <MenuLink href="/import" icon={FileUp}>{t("nav.import")}</MenuLink>}
              {isAdmin && <MenuLink href="/admin/users" icon={Users}>{t("nav.users")}</MenuLink>}
              <Dropdown.Separator className="menu-sep" />
              <Dropdown.Item className="menu-item" onSelect={() => void signOut()}>
                <LogOut size={16} aria-hidden="true" />
                {t("nav.signOut")}
              </Dropdown.Item>
            </Dropdown.Content>
          </Dropdown.Portal>
        </Dropdown.Root>
      )}
      </div>
    </header>
  );
}
