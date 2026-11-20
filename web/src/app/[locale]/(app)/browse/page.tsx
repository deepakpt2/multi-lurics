import type { Metadata } from "next";
import { permanentRedirect } from "next/navigation";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { BrowseView } from "@/components/browse/browse";
import { browseHref, fetchBrowse, parseState, toQuery, type SongsResponse } from "@/lib/browse";
import { siteUrl } from "@/lib/seo";

// The address is the state (filters, page, size): render per request, never from a build-time copy.
export const dynamic = "force-dynamic";

type Props = {
  params: Promise<{ locale: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
};

/** The state as the server understood it: each filter under its current slug, once. */
function canonicalState(state: ReturnType<typeof parseState>, data: SongsResponse) {
  const seen = new Set<string>();
  const filters = data.filters
    .map((f) => ({ facet: f.facet, slug: f.slug }))
    .filter((f) => !seen.has(`${f.facet}:${f.slug}`) && seen.add(`${f.facet}:${f.slug}`));
  return { ...state, filters };
}

export async function generateMetadata({ params, searchParams }: Props): Promise<Metadata> {
  const { locale } = await params;
  const state = parseState(await searchParams);
  const [t, r] = await Promise.all([getTranslations({ locale, namespace: "browse" }), fetchBrowse<SongsResponse>("songs", toQuery(state), locale)]);
  const data = r.ok ? r.data : null;
  const names = data?.filters.filter((f) => f.resolved).map((f) => f.label.text).join(", ");
  const canonical = data ? `${siteUrl()}/${locale}${browseHref(canonicalState(state, data))}` : undefined;
  return {
    title: names ? t("byTitle", { filters: names }) : t("title"),
    description: t("metaDescription"),
    alternates: canonical ? { canonical } : undefined,
    // A list with nothing in it, or a filter that does not exist, is not worth indexing.
    robots: { index: Boolean(data && data.total > 0 && data.filters.every((f) => f.resolved)), follow: true },
  };
}

export default async function Page({ params, searchParams }: Props) {
  const { locale } = await params;
  setRequestLocale(locale);
  const state = parseState(await searchParams);
  const t = await getTranslations("browse");
  const result = await fetchBrowse<SongsResponse>("songs", toQuery(state), locale);
  if (result.ok) {
    // A renamed person or album keeps working: send the old address to the current one for good.
    const canonical = canonicalState(state, result.data);
    if (canonical.filters.length === state.filters.length && toQuery(canonical) !== toQuery(state)) {
      permanentRedirect(`/${locale}${browseHref(canonical)}`);
    }
  }
  return (
    <>
      <h1>{t("title")}</h1>
      <BrowseView state={state} result={result} />
    </>
  );
}
