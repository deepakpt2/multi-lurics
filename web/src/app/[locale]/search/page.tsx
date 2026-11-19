import { getTranslations, setRequestLocale } from "next-intl/server";
import { SearchResults } from "@/components/search/search";

export default async function Page({
  params,
  searchParams,
}: {
  params: Promise<{ locale: string }>;
  searchParams: Promise<{ q?: string | string[] }>;
}) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("search");
  const { q } = await searchParams;
  return (
    <>
      <h1>{t("title")}</h1>
      <SearchResults initialQuery={(Array.isArray(q) ? q[0] : q) ?? ""} />
    </>
  );
}
