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
      <p>
        <Link href="/moderation/lyrics">{t("lyricsQueueLink")}</Link>
        {" "}
        <Link href="/moderation/entities">{t("proposalsQueueLink")}</Link>
        {" "}
        <Link href="/moderation/community">{tc("reportsTitle")}</Link>
        {" "}
        <Link href="/moderation/legal">{tl("queueTitle")}</Link>
      </p>
      <ModerationQueue />
    </>
  );
}
