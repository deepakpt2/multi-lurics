import { useTranslations } from "next-intl";
import { Link } from "@/i18n/navigation";
import { LocaleSwitcher } from "./locale-switcher";

export function SiteFooter() {
  const t = useTranslations("nav");
  return (
    <footer className="site-footer">
      <nav aria-label={t("legal")}>
        <Link href="/legal/terms">{t("terms")}</Link>
        <Link href="/legal/privacy">{t("privacy")}</Link>
        <Link href="/legal/content-policy">{t("contentPolicy")}</Link>
        <Link href="/legal/notice">{t("copyright")}</Link>
      </nav>
      <LocaleSwitcher />
    </footer>
  );
}
