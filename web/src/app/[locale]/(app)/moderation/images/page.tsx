import { getTranslations, setRequestLocale } from "next-intl/server";
import { ImageQueue } from "@/components/media/image-queue";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("images");
  return (
    <>
      <h1>{t("queueTitle")}</h1>
      <ImageQueue />
    </>
  );
}
