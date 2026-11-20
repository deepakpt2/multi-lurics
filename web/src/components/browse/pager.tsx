"use client";

import { ChevronLeft, ChevronRight } from "lucide-react";
import { useTranslations } from "next-intl";
import { Link, useRouter } from "@/i18n/navigation";
import { PAGE_SIZES, browseHref, pageWindow, type BrowseState } from "@/lib/browse";

/**
 * Page links (real links, so crawlers and "open in new tab" work), previous/next, and the page size.
 * Everything it changes lives in the address, so back, forward and refresh behave.
 */
export function Pager({
  path,
  state,
  pages,
  withSort = false,
  extra = {},
}: {
  path: string;
  state: BrowseState;
  pages: number;
  withSort?: boolean;
  /** Query parameters that are not part of the browse state, such as a search text. */
  extra?: Record<string, string>;
}) {
  const t = useTranslations("browse.pager");
  const router = useRouter();
  const href = (patch: Partial<BrowseState>) => browseHref({ ...state, ...patch }, path, extra);
  const go = (patch: Partial<BrowseState>) => router.push(href(patch));
  return (
    <div className="pager">
      {pages > 1 && (
        <nav aria-label={t("label")}>
          <ul>
            <li>
              {state.page > 1 ? (
                <Link href={href({ page: state.page - 1 })} rel="prev" className="button">
                  <ChevronLeft size={16} aria-hidden="true" />
                  {t("previous")}
                </Link>
              ) : (
                <span className="button" aria-disabled="true">
                  <ChevronLeft size={16} aria-hidden="true" />
                  {t("previous")}
                </span>
              )}
            </li>
            {pageWindow(state.page, pages).map((n, i) =>
              n === null ? (
                <li key={`gap-${i}`} aria-hidden="true">
                  {t("gap")}
                </li>
              ) : (
                <li key={n}>
                  <Link href={href({ page: n })} className="button" aria-current={n === state.page ? "page" : undefined} aria-label={t("page", { page: n })}>
                    {n}
                  </Link>
                </li>
              ),
            )}
            <li>
              {state.page < pages ? (
                <Link href={href({ page: state.page + 1 })} rel="next" className="button">
                  {t("next")}
                  <ChevronRight size={16} aria-hidden="true" />
                </Link>
              ) : (
                <span className="button" aria-disabled="true">
                  {t("next")}
                  <ChevronRight size={16} aria-hidden="true" />
                </span>
              )}
            </li>
          </ul>
        </nav>
      )}
      <p className="muted small" role="status">
        {pages > 0 ? t("status", { page: Math.min(state.page, pages), pages }) : null}
      </p>
      <div className="pager-options">
        <label>
          {t("perPage")}
          <select value={state.pageSize} onChange={(e) => go({ pageSize: Number(e.target.value), page: 1 })}>
            {PAGE_SIZES.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </label>
        {withSort && (
          <label>
            {t("sort")}
            <select value={state.sort} onChange={(e) => go({ sort: e.target.value as BrowseState["sort"], page: 1 })}>
              <option value="newest">{t("newest")}</option>
              <option value="title">{t("title")}</option>
            </select>
          </label>
        )}
      </div>
    </div>
  );
}
