import { getTranslations, setRequestLocale } from "next-intl/server";
import { ArticleEditor } from "@/components/articles/article-editor";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("articles");
  return (
    <>
      <h1>{t("new")}</h1>
      <ArticleEditor />
    </>
  );
}
