import { getTranslations, setRequestLocale } from "next-intl/server";
import { ArticleList } from "@/components/articles/article-list";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("articles");
  return (
    <>
      <h1>{t("title")}</h1>
      <p className="lead">{t("intro")}</p>
      <ArticleList />
    </>
  );
}
