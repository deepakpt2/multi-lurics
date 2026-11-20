import type { Metadata } from "next";
import { notFound, permanentRedirect } from "next/navigation";
import { setRequestLocale } from "next-intl/server";
import { MetadataPage } from "@/components/browse/metadata-page";
import { fetchBrowse, isFacet, itemHref, parseState, type SongsResponse } from "@/lib/browse";
import { siteUrl } from "@/lib/seo";

type Props = {
  params: Promise<{ locale: string; facet: string; slug: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
};

const decode = (s: string) => {
  try {
    return decodeURIComponent(s).normalize("NFC");
  } catch {
    return s;
  }
};

/** Songs of one value, grouped by album; the page size is the largest the API allows. */
async function load(locale: string, facet: string, slug: string, page: number) {
  const q = new URLSearchParams({ [facet]: slug, sort: "album", page_size: "50", page: String(page) });
  return fetchBrowse<SongsResponse>("songs", q.toString(), locale);
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { locale, facet, slug } = await params;
  if (!isFacet(facet)) return { robots: { index: false } };
  const r = await load(locale, facet, decode(slug), 1);
  const f = r.ok ? r.data.filters[0] : undefined;
  if (!f?.resolved) return { robots: { index: false } };
  return {
    title: f.label.text,
    alternates: { canonical: `${siteUrl()}/${locale}${itemHref(facet, f.slug)}` },
    robots: { index: true, follow: true },
  };
}

export default async function Page({ params, searchParams }: Props) {
  const { locale, facet, slug: raw } = await params;
  if (!isFacet(facet)) notFound();
  setRequestLocale(locale);
  const slug = decode(raw);
  const state = parseState(await searchParams);
  const result = await load(locale, facet, slug, state.page);
  if (result.ok) {
    const f = result.data.filters[0];
    if (!f?.resolved) notFound();
    // A renamed value keeps its old address: send it to the current one.
    if (f.slug !== slug) permanentRedirect(`/${locale}${itemHref(facet, f.slug)}`);
  }
  return <MetadataPage facet={facet} slug={slug} state={state} result={result} />;
}
