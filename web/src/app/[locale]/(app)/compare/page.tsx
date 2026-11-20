import { getTranslations, setRequestLocale } from "next-intl/server";
import { Compare, parseIds } from "@/components/compare/compare";

export default async function Page({ params, searchParams }: { params: Promise<{ locale: string }>; searchParams: Promise<{ c?: string }> }) {
  const { locale } = await params;
  const { c } = await searchParams;
  setRequestLocale(locale);
  const t = await getTranslations("compare");
  return (
    <>
      <h1>{t("title")}</h1>
      <p className="muted">{t("intro")}</p>
      <Compare initialIds={parseIds(c)} />
    </>
  );
}
