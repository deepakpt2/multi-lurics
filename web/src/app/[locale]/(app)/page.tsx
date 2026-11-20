import { Library, LogIn, UserPlus } from "lucide-react";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { Link } from "@/i18n/navigation";
import { HomeSections } from "@/components/home/home-sections";
import { HomeArticles } from "@/components/articles/home-articles";
import { GuestOnly } from "@/components/guest-only";

export default async function Home({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations();
  return (
    <>
    <section className="hero">
      <h1>{t("home.title")}</h1>
      <p className="lead">{t("app.tagline")}</p>
      <GuestOnly>
        <p>{t("home.intro")}</p>
      </GuestOnly>
      <p className="actions">
        <GuestOnly>
        <Link className="button primary" href="/register">
          <UserPlus size={16} aria-hidden="true" />
          {t("nav.register")}
        </Link>
        <Link className="button" href="/login">
          <LogIn size={16} aria-hidden="true" />
          {t("nav.signIn")}
        </Link>
        </GuestOnly>
        <Link className="button ghost" href="/browse">
          <Library size={16} aria-hidden="true" />
          {t("home.browse")}
        </Link>
      </p>
    </section>
    <HomeArticles />
    <HomeSections />
    </>
  );
}
