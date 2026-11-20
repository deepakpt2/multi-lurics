import { setRequestLocale } from "next-intl/server";
import { ArticleView } from "@/components/articles/article-view";

export default async function Page({ params }: { params: Promise<{ locale: string; slug: string }> }) {
  const { locale, slug } = await params;
  setRequestLocale(locale);
  return <ArticleView slug={decodeURIComponent(slug)} />;
}
