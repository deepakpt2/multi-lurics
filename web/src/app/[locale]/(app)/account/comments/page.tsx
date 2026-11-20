import { getTranslations, setRequestLocale } from "next-intl/server";
import { MyComments } from "@/components/community/community";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("community");
  return (
    <>
      <h1>{t("myComments")}</h1>
      <MyComments />
    </>
  );
}
