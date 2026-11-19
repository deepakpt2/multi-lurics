import { getTranslations, setRequestLocale } from "next-intl/server";
import { Link } from "@/i18n/navigation";
import { ModerationQueue } from "@/components/localization/moderation";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("localization");
  return (
    <>
      <h1>{t("queueTitle")}</h1>
      <p>
        <Link href="/moderation/lyrics">{t("lyricsQueueLink")}</Link>
      </p>
      <ModerationQueue />
    </>
  );
}
