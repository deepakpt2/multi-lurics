import { FileText, Flag, GitPullRequest, Scale } from "lucide-react";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { Link } from "@/i18n/navigation";
import { ModerationQueue } from "@/components/localization/moderation";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("localization");
  const tc = await getTranslations("community");
  const tl = await getTranslations("legal");
  return (
    <>
      <h1>{t("queueTitle")}</h1>
      <nav className="hub" aria-label={t("queueTitle")}>
        <Link href="/moderation/lyrics" className="hub-card">
          <FileText size={22} aria-hidden="true" />
          <span>{t("lyricsQueueLink")}</span>
        </Link>
        <Link href="/moderation/entities" className="hub-card">
          <GitPullRequest size={22} aria-hidden="true" />
          <span>{t("proposalsQueueLink")}</span>
        </Link>
        <Link href="/moderation/community" className="hub-card">
          <Flag size={22} aria-hidden="true" />
          <span>{tc("reportsTitle")}</span>
        </Link>
        <Link href="/moderation/legal" className="hub-card">
          <Scale size={22} aria-hidden="true" />
          <span>{tl("queueTitle")}</span>
        </Link>
      </nav>
      <ModerationQueue />
    </>
  );
}
