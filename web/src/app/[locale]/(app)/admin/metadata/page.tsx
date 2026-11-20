import { getTranslations, setRequestLocale } from "next-intl/server";
import { MetadataAdmin } from "@/components/admin/metadata";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("admin.metadata");
  return (
    <>
      <h1>{t("title")}</h1>
      <p className="lead">{t("intro")}</p>
      <MetadataAdmin />
    </>
  );
}
