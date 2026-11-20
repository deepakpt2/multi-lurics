"use client";

import { useTranslations } from "next-intl";
import { useEffect, useState, useTransition } from "react";
import { Link, useRouter } from "@/i18n/navigation";
import { toProblem } from "@/api/client";
import {
  FACET_KEYS,
  browseHref,
  itemHref,
  type BrowseSong,
  type BrowseState,
  type Loaded,
  type SongsResponse,
} from "@/lib/browse";
import { Name } from "../localization/localized";
import { EmptyState, ErrorState } from "../states";
import { EntityImage } from "../media/entity-image";
import { FACET_ICON } from "./facets";
import { FilterSearch } from "./filter-search";
import { Pager } from "./pager";

/** Links to the pages that list one kind of metadata. */
export function FacetNav({ current }: { current?: string }) {
  const t = useTranslations("browse");
  return (
    <nav aria-label={t("browseBy")} className="facet-nav">
      <ul>
        {FACET_KEYS.map((k) => {
          const Icon = FACET_ICON[k];
          return (
            <li key={k}>
              <Link
                href={`/browse/${k}`}
                aria-current={current === k ? "page" : undefined}
              >
                <Icon size={16} aria-hidden="true" />
                {t(`facets.${k}`)}
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

function SongCard({ song }: { song: BrowseSong }) {
  const t = useTranslations("browse.results");
  // Each singer and album has a page of its own, with its songs grouped by album.
  const filterHref = (facet: "singer" | "album", slug: string) => itemHref(facet, slug);
  // The album's cover if the song is on one, otherwise its first singer's photo.
  const cover = song.albums[0]
    ? { type: "album" as const, ref: song.albums[0] }
    : song.singers[0]
      ? { type: "artist" as const, ref: song.singers[0] }
      : undefined;
  return (
    <li className="song-card">
      {cover && (
        <EntityImage
          type={cover.type}
          id={cover.ref.id}
          imageId={cover.ref.image_id}
          name={cover.ref.name.text}
          size={64}
        />
      )}
      <div className="song-card-body">
        <h2>
          {song.page ? (
            <Link
              href={`/song/${encodeURIComponent(song.page.slug)}`}
              locale={song.page.locale as never}
            >
              <Name display={song.title} />
            </Link>
          ) : (
            <>
              <Name display={song.title} />{" "}
              <small className="badge">{t("noPage")}</small>
            </>
          )}
          {song.year && (
            <small className="muted">
              {" "}
              {t("year", { year: String(song.year) })}
            </small>
          )}
        </h2>
        <dl className="song-meta">
          {song.singers.length > 0 && (
            <div>
              <dt>{t("singers")}</dt>
              <dd>
                {song.singers.map((s, i) => (
                  <span key={s.slug}>
                    {i > 0 && ", "}
                    <Link href={filterHref("singer", s.slug)}>
                      <Name display={s.name} />
                    </Link>
                  </span>
                ))}
              </dd>
            </div>
          )}
          {song.albums.length > 0 && (
            <div>
              <dt>{t("albums")}</dt>
              <dd>
                {song.albums.map((a, i) => (
                  <span key={a.slug}>
                    {i > 0 && ", "}
                    <Link href={filterHref("album", a.slug)}>
                      <Name display={a.name} />
                    </Link>
                  </span>
                ))}
              </dd>
            </div>
          )}
          {song.original_language && (
            <div>
              <dt>{t("language")}</dt>
              <dd lang={song.original_language.code}>
                {song.original_language.name}
              </dd>
            </div>
          )}
          <div>
            <dt>{t("translations")}</dt>
            <dd>
              {song.translations.length ? (
                song.translations.map((l) => l.name).join(", ")
              ) : (
                <span className="muted">{t("noTranslations")}</span>
              )}
            </dd>
          </div>
        </dl>
      </div>
    </li>
  );
}

/**
 * The browse page. The address is the whole state (filters, page, page size, sort); the server reads
 * it and sends the matching songs, so refresh, share, back and forward all restore the same view.
 * Changing a filter or page only changes the address.
 */
export function BrowseView({
  state,
  result,
}: {
  state: BrowseState;
  result: Loaded<SongsResponse>;
}) {
  const t = useTranslations("browse");
  const router = useRouter();
  const [pending, startTransition] = useTransition();
  // The chips follow a click at once; the list follows when the server answers.
  const [next, setNext] = useState<BrowseState | null>(null);
  const key = browseHref(state);
  useEffect(() => setNext(null), [key]);
  const shown = next ?? state;
  const go = (s: BrowseState) => {
    setNext(s);
    startTransition(() => router.push(browseHref(s)));
  };
  const data = result.ok ? result.data : null;
  const unresolved = data?.filters.filter((f) => !f.resolved) ?? [];
  const pages = data?.total_pages ?? 0;
  const hasFilters = state.filters.length > 0;

  return (
    <>
      <FilterSearch
        state={shown}
        filters={data?.filters ?? []}
        onChange={go}
        busy={pending}
      />
      <FacetNav />
      <section
        aria-labelledby="results-title"
        aria-busy={pending || undefined}
        data-pending={pending || undefined}
      >
        <h2 id="results-title" className="results-title">
          {data ? t("results.count", { count: data.total }) : t("results.list")}
          {pending && <small className="muted"> {t("results.updating")}</small>}
        </h2>
        {!result.ok && (
          <ErrorState
            error={toProblem(null, result.status)}
            onRetry={() => router.refresh()}
          />
        )}
        {unresolved.map((f) => (
          <p
            key={`${f.facet}:${f.slug}`}
            className="alert alert-warning"
            role="status"
          >
            {t("results.unresolved", { name: f.slug })}
          </p>
        ))}
        {data && data.items.length === 0 && (
          <EmptyState
            title={hasFilters ? t("results.noMatch") : t("results.empty")}
          >
            {hasFilters && <p className="muted">{t("results.noMatchHint")}</p>}
          </EmptyState>
        )}
        {data && data.items.length > 0 && (
          <ol
            className="song-list"
            aria-label={t("results.list")}
            start={(data.page - 1) * data.page_size + 1}
          >
            {data.items.map((s) => (
              <SongCard key={`${s.page?.slug ?? s.title.text}`} song={s} />
            ))}
          </ol>
        )}
        {data && <Pager path="/browse" state={state} pages={pages} withSort />}
      </section>
    </>
  );
}
