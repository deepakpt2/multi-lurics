import { getTranslations, setRequestLocale } from "next-intl/server";
import { ContributionEdit } from "@/components/lyrics/lyrics";

export default async function Page({ params }: { params: Promise<{ locale: string; id: string }> }) {
  const { locale, id } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("lyrics");
  return (
    <>
      <h1>{t("editTitle")}</h1>
      <ContributionEdit id={id} />
    </>
  );
}
