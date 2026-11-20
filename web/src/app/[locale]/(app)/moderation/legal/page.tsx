import { getTranslations, setRequestLocale } from "next-intl/server";
import { NoticeQueue } from "@/components/legal/legal";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("legal");
  return (
    <>
      <h1>{t("queueTitle")}</h1>
      <NoticeQueue />
    </>
  );
}
