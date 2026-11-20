import { getTranslations, setRequestLocale } from "next-intl/server";
import { MyContributions } from "@/components/lyrics/lyrics";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("lyrics");
  return (
    <>
      <h1>{t("mineTitle")}</h1>
      <MyContributions />
    </>
  );
}
