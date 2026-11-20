import { getTranslations, setRequestLocale } from "next-intl/server";
import { CorrectionsQueue } from "@/components/lyrics/corrections-queue";
import { LyricsQueue } from "@/components/lyrics/lyrics";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("lyrics");
  const tc = await getTranslations("corrections");
  return (
    <>
      <h1>{t("queueTitle")}</h1>
      <LyricsQueue />
      <h2>{tc("title")}</h2>
      <CorrectionsQueue />
    </>
  );
}
