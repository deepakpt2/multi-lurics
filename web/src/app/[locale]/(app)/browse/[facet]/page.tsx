import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { FacetList } from "@/components/browse/facet-list";
import { fetchBrowse, isFacet, parseState, toQuery, type FacetResponse } from "@/lib/browse";
import { siteUrl } from "@/lib/seo";

type Props = {
  params: Promise<{ locale: string; facet: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
};

const qOf = (v: string | string[] | undefined) => (Array.isArray(v) ? v[0] : v)?.trim() ?? "";

export async function generateMetadata({ params, searchParams }: Props): Promise<Metadata> {
  const { locale, facet } = await params;
  if (!isFacet(facet)) return { robots: { index: false } };
  const sp = await searchParams;
  const t = await getTranslations({ locale, namespace: "browse" });
  return {
    title: t("facetPage.title", { facets: t(`facets.${facet}`) }),
    description: t("metaDescription"),
    // The plain list is the page to index; searches and filtered views are not.
    alternates: { canonical: `${siteUrl()}/${locale}/browse/${facet}` },
    robots: { index: parseState(sp).filters.length === 0 && !qOf(sp.q), follow: true },
  };
}

export default async function Page({ params, searchParams }: Props) {
  const { locale, facet } = await params;
  if (!isFacet(facet)) notFound();
  setRequestLocale(locale);
  const sp = await searchParams;
  const state = parseState(sp);
  const q = qOf(sp.q);
  const result = await fetchBrowse<FacetResponse>(`facets/${facet}`, toQuery(state, { q }), locale);
  return <FacetList facet={facet} state={state} q={q} result={result} />;
}
