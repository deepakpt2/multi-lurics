import { getTranslations, setRequestLocale } from "next-intl/server";
import { ContributionHistory } from "@/components/lyrics/lyrics";

export default async function Page({ params }: { params: Promise<{ locale: string; id: string }> }) {
  const { locale, id } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("lyrics");
  return (
    <>
      <h1>{t("historyTitle")}</h1>
      <ContributionHistory id={id} />
    </>
  );
}
