import { getTranslations, setRequestLocale } from "next-intl/server";
import { AppealQueue, ReportQueue } from "@/components/community/moderation";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("community");
  return (
    <>
      <h1>{t("reportsTitle")}</h1>
      <ReportQueue />
      <h2>{t("appealsTitle")}</h2>
      <AppealQueue />
    </>
  );
}
