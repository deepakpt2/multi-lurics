import { Suspense } from "react";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { NoticeForm } from "@/components/legal/legal";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("legal");
  return (
    <>
      <h1>{t("noticeTitle")}</h1>
      <p>{t("noticeIntro")}</p>
      <Suspense>
        <NoticeForm />
      </Suspense>
    </>
  );
}
