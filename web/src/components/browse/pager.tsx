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
        <div className="segmented" role="radiogroup" aria-label={t("perPage")}>
          <span className="segmented-label" aria-hidden="true">{t("perPage")}</span>
          {PAGE_SIZES.map((n) => (
            <button key={n} type="button" role="radio" aria-checked={state.pageSize === n} onClick={() => go({ pageSize: n, page: 1 })}>
              {n}
            </button>
          ))}
        </div>
        {withSort && (
          <div className="segmented" role="radiogroup" aria-label={t("sort")}>
            <span className="segmented-label" aria-hidden="true">{t("sort")}</span>
            {(["newest", "title"] as const).map((k) => (
              <button key={k} type="button" role="radio" aria-checked={state.sort === k} onClick={() => go({ sort: k, page: 1 })}>
                {t(k)}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
