import { getTranslations, setRequestLocale } from "next-intl/server";
import { BulkImport } from "@/components/import/bulk-import";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("import");
  return (
    <>
      <h1>{t("title")}</h1>
      <p className="lead">{t("intro")}</p>
      <BulkImport />
    </>
  );
}
