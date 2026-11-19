import { getTranslations, setRequestLocale } from "next-intl/server";
import { Link } from "@/i18n/navigation";

export default async function Home({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations();
  return (
    <>
      <h1>{t("home.title")}</h1>
      <p className="lead">{t("app.tagline")}</p>
      <p>{t("home.intro")}</p>
      <p className="actions">
        <Link className="button primary" href="/register">
          {t("nav.register")}
        </Link>
        <Link className="button" href="/login">
          {t("nav.signIn")}
        </Link>
      </p>
    </>
  );
}
