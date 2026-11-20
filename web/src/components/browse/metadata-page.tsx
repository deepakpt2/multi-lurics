"use client";

import { ListFilter } from "lucide-react";
import { useTranslations } from "next-intl";
import { Link } from "@/i18n/navigation";
import { toProblem } from "@/api/client";
import { browseHref, itemHref, type BrowseSong, type BrowseState, type FacetKey, type Loaded, type SongsResponse } from "@/lib/browse";
import { EntityImage } from "../media/entity-image";
import { ContributeButton } from "../public/contribute";
import { Name } from "../localization/localized";
import { EmptyState, ErrorState } from "../states";
import { FACET_ICON, IMAGE_OF } from "./facets";
import { FacetNav } from "./browse";
import { useValueLabel } from "./filter-search";
import { Pager } from "./pager";

type Group = { key: string; album: BrowseSong["albums"][number] | null; songs: BrowseSong[] };

/** Songs in the order served (grouped by album), split wherever the first album changes. */
export function groupByAlbum(songs: BrowseSong[]): Group[] {
  const groups: Group[] = [];
  for (const s of songs) {
    const album = s.albums[0] ?? null;
    const key = album?.slug ?? "";
    const last = groups[groups.length - 1];
    if (last && last.key === key) last.songs.push(s);
    else groups.push({ key, album, songs: [s] });
  }
  return groups;
}

function SongRow({ song }: { song: BrowseSong }) {
  const t = useTranslations("browse.results");
  return (
    <li className="meta-song">
      <span className="meta-song-title">
        {song.page ? (
          <Link href={`/song/${encodeURIComponent(song.page.slug)}`} locale={song.page.locale as never}>
            <Name display={song.title} />
          </Link>
        ) : (
          <>
            <Name display={song.title} /> <small className="badge">{t("noPage")}</small>
          </>
        )}
        {song.year && <small className="muted"> {t("year", { year: String(song.year) })}</small>}
      </span>
      {song.singers.length > 0 && (
        <span className="muted small">
          {song.singers.map((s, i) => (
            <span key={s.slug}>
              {i > 0 && ", "}
              <Link href={itemHref("singer", s.slug)}>
                <Name display={s.name} />
              </Link>
            </span>
          ))}
        </span>
      )}
    </li>
  );
}

/**
 * The page of one value: a singer, a film (an album), a raga, a deity… With its picture and basic facts,
 * and every song that has it, grouped by album the way other lyric catalogues list them.
 */
export function MetadataPage({
  facet,
  slug,
  state,
  result,
}: {
  facet: FacetKey;
  slug: string;
  state: BrowseState;
  result: Loaded<SongsResponse>;
}) {
  const t = useTranslations("browse");
  const tc = useTranslations("catalogue");
  const label = useValueLabel();
  const Icon = FACET_ICON[facet];
  const data = result.ok ? result.data : null;
  const filter = data?.filters[0];
  const path = itemHref(facet, slug);
  const name = filter ? label(facet, filter.label) : slug;
  const imageType = IMAGE_OF[facet];
  const people = facet === "artist" || facet === "singer" || facet === "lyricist" || facet === "composer";
  const kind = filter?.kind
    ? people
      ? tc(`artistKinds.${filter.kind as "person"}`)
      : facet === "album"
        ? tc(`albumKinds.${filter.kind as "album"}`)
        : undefined
    : undefined;
  const facts = [kind, filter?.detail].filter((x): x is string => !!x);
  const groups = facet === "album" || !data ? null : groupByAlbum(data.items);
  const target = filter?.id && (people || facet === "album") ? ({ kind: facet === "album" ? "album" : "artist", id: filter.id } as const) : null;

  return (
    <article className="meta-page">
      <FacetNav current={facet} />
      {!result.ok && <ErrorState error={toProblem(null, result.status)} />}
      {filter && (
        <header className="meta-hero">
          {imageType ? (
            <EntityImage type={imageType} id={filter.id ?? undefined} imageId={filter.image_id} name={name} size={120} />
          ) : (
            <span className="meta-icon" aria-hidden="true">
              <Icon size={48} />
            </span>
          )}
          <div>
            <p className="muted small">{t(`facets.${facet}`)}</p>
            <h1>
              <Name display={{ ...filter.label, text: name }} />
            </h1>
            {facts.length > 0 && <p className="muted">{facts.join(" · ")}</p>}
            <p className="muted">{t("item.songs", { count: data?.total ?? 0 })}</p>
            <p className="page-actions">
              {target && <ContributeButton target={target} />}
              <Link className="button" href={browseHref({ filters: [{ facet, slug }] })}>
                <ListFilter size={16} aria-hidden="true" /> {t("item.filter")}
              </Link>
            </p>
          </div>
        </header>
      )}
      {data && data.items.length === 0 && <EmptyState title={t("item.empty")} />}
      {data && groups && groups.map((g) => (
        <section key={g.key || "none"} className="album-group" aria-labelledby={`g-${g.key || "none"}`}>
          <h2 id={`g-${g.key || "none"}`} className="album-group-title">
            {g.album ? (
              <>
                <EntityImage type="album" imageId={g.album.image_id} name={g.album.name.text} size={40} />
                <Link href={itemHref("album", g.album.slug)}>
                  <Name display={g.album.name} />
                </Link>
              </>
            ) : (
              t("item.noAlbum")
            )}
          </h2>
          <ol className="meta-songs">{g.songs.map((s, i) => <SongRow key={`${s.page?.slug ?? s.title.text}-${i}`} song={s} />)}</ol>
        </section>
      ))}
      {data && !groups && data.items.length > 0 && <ol className="meta-songs">{data.items.map((s, i) => <SongRow key={`${s.page?.slug ?? s.title.text}-${i}`} song={s} />)}</ol>}
      {data && <Pager path={path} state={{ ...state, filters: [] }} pages={data.total_pages} />}
    </article>
  );
}
