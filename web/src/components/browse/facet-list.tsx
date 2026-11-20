"use client";

import { useTranslations } from "next-intl";
import { Link } from "@/i18n/navigation";
import { toProblem } from "@/api/client";
import { browseHref, type BrowseState, type FacetKey, type FacetResponse, type Loaded } from "@/lib/browse";
import { Name } from "../localization/localized";
import { EmptyState, ErrorState } from "../states";
import { FacetNav } from "./browse";
import { FACET_ICON } from "./facets";
import { useValueLabel } from "./filter-search";
import { Pager } from "./pager";

/**
 * One kind of metadata, paged. Each entry links to the songs that have it (added to any filters the
 * visitor already chose), so "all albums of this singer" is the same page with a singer filter.
 */
export function FacetList({
  facet,
  state,
  q,
  result,
}: {
  facet: FacetKey;
  state: BrowseState;
  q: string;
  result: Loaded<FacetResponse>;
}) {
  const t = useTranslations("browse");
  const label = useValueLabel();
  const Icon = FACET_ICON[facet];
  const path = `/browse/${facet}`;
  const data = result.ok ? result.data : null;
  return (
    <>
      <h1>
        <Icon size={24} aria-hidden="true" /> {t("facetPage.title", { facets: t(`facets.${facet}`) })}
      </h1>
      <FacetNav current={facet} />
      <form className="search-form" role="search" action={path} method="get">
        {state.filters.map((f) => (
          <input key={`${f.facet}:${f.slug}`} type="hidden" name={f.facet} value={f.slug} />
        ))}
        <label htmlFor="facet-q">{t("facetPage.filter", { facets: t(`facets.${facet}`) })}</label>
        <input id="facet-q" name="q" type="search" dir="auto" autoComplete="off" defaultValue={q} />
        <button type="submit" className="button primary">
          {t("facetPage.submit")}
        </button>
      </form>
      {state.filters.length > 0 && (
        <p className="muted">
          {t("facetPage.inContext", { facets: t(`facets.${facet}`) })} <Link href={browseHref(state)}>{t("facetPage.allSongs")}</Link>
        </p>
      )}
      {!result.ok && <ErrorState error={toProblem(null, result.status)} />}
      {data && data.items.length === 0 && <EmptyState title={q ? t("facetPage.noMatch", { q }) : t("facetPage.empty")} />}
      {data && data.items.length > 0 && (
        <ul className="facet-values" aria-label={t(`facets.${facet}`)}>
          {data.items.map((v) => (
            <li key={v.slug}>
              <Link href={browseHref({ ...state, filters: [...state.filters, { facet, slug: v.slug }], page: 1 })}>
                <Name display={{ ...v.label, text: label(facet, v.label) }} />
              </Link>
              <small className="muted">{t("facetPage.songs", { count: v.songs })}</small>
            </li>
          ))}
        </ul>
      )}
      {data && <Pager path={path} state={state} pages={data.total_pages} extra={{ q }} />}
    </>
  );
}
