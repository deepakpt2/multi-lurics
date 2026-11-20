"use client";

import { useCallback, useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { ArrowDown, ArrowUp, Eye, EyeOff, GripVertical, MessageCircle, RotateCcw, Settings2, Star, X } from "lucide-react";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Link } from "@/i18n/navigation";
import { browseHref } from "@/lib/browse";
import { DEFAULT_PREFS, hide, loadPrefs, move, moveTo, savePrefs, show, visible, type Prefs, type SectionKey } from "@/lib/home-prefs";
import { Name } from "../localization/localized";
import { EntityImage } from "../media/entity-image";
import { useSession } from "../session";
import { ErrorState, LoadingState } from "../states";

type Home = components["schemas"]["HomeResponse"];
type Item = components["schemas"]["HomeItem"];

const ADD_HREF: Record<SectionKey, string> = {
  editors_picks: "/compositions/new",
  popular_songs: "/compositions/new",
  new_songs: "/compositions/new",
  popular_albums: "/albums/new",
  new_albums: "/albums/new",
  popular_artists: "/artists/new",
};

const filterHref = (facet: "singer" | "album" | "artist", slug: string) => browseHref({ filters: [{ facet, slug }] });

function MetaLinks({ facet, refs }: { facet: "singer" | "album"; refs: Item["singers"] }) {
  return (
    <>
      {refs.map((r, i) => (
        <span key={r.slug}>
          {i > 0 && ", "}
          <Link href={filterHref(facet, r.slug)}>
            <Name display={r.name} />
          </Link>
        </span>
      ))}
    </>
  );
}

/**
 * Songs link to their own page. Everything else (an album, an artist, a song's singers and albums)
 * is metadata and links to the browse page of all lyrics under that value.
 */
function ItemRow({ section, item, onUnpick }: { section: SectionKey; item: Item; onUnpick?: () => void }) {
  const t = useTranslations("home");
  const isSong = section.endsWith("songs") || section === "editors_picks";
  const isAlbum = section.endsWith("albums");
  const title = <Name display={item.title} />;
  return (
    <li className="home-item">
      <span className="home-rank" aria-hidden="true">
        {item.rank}
      </span>
      {!isSong && <EntityImage type={isAlbum ? "album" : "artist"} id={item.id} imageId={item.image_id} name={item.title.text} size={48} />}
      <div className="home-item-body">
        {isSong ? (
          item.page ? (
            <Link href={`/song/${encodeURIComponent(item.page.slug)}`} locale={item.page.locale as never}>
              {title}
            </Link>
          ) : (
            title
          )
        ) : item.slug ? (
          <Link href={filterHref(isAlbum ? "album" : "artist", item.slug)}>{title}</Link>
        ) : (
          title
        )}
        {isSong && (item.singers.length > 0 || item.albums.length > 0) && (
          <span className="muted small">
            <MetaLinks facet="singer" refs={item.singers} />
            {item.singers.length > 0 && item.albums.length > 0 && " · "}
            <MetaLinks facet="album" refs={item.albums} />
          </span>
        )}
        {section.startsWith("popular") && (
          <span className="home-stats muted small">
            <span title={t("views", { count: item.views })}>
              <Eye size={13} aria-hidden="true" /> {t("viewsShort", { count: item.views })}
            </span>
            {isSong && item.rating_count > 0 && item.rating_average != null && (
              <span title={t("ratings", { count: item.rating_count })}>
                <Star size={13} aria-hidden="true" /> {t("ratingShort", { average: item.rating_average.toFixed(1), count: item.rating_count })}
              </span>
            )}
            {isSong && item.comments > 0 && (
              <span title={t("comments", { count: item.comments })}>
                <MessageCircle size={13} aria-hidden="true" /> {item.comments}
              </span>
            )}
          </span>
        )}
      </div>
      {onUnpick && (
        <button type="button" className="icon-button home-unpick" onClick={onUnpick} aria-label={t("unpick", { name: item.title.text })} title={t("unpick", { name: item.title.text })}>
          <X size={16} aria-hidden="true" />
        </button>
      )}
    </li>
  );
}

function Controls({ k, index, last, onMove, onHide }: { k: SectionKey; index: number; last: number; onMove: (d: -1 | 1) => void; onHide: () => void }) {
  const t = useTranslations("home");
  const name = t(`sections.${k}.title`);
  return (
    <span className="home-controls">
      <GripVertical size={16} aria-hidden="true" className="home-grip" />
      <button type="button" className="icon-button" disabled={index === 0} onClick={() => onMove(-1)} aria-label={t("moveUp", { name })} title={t("moveUp", { name })}>
        <ArrowUp size={16} aria-hidden="true" />
      </button>
      <button type="button" className="icon-button" disabled={index === last} onClick={() => onMove(1)} aria-label={t("moveDown", { name })} title={t("moveDown", { name })}>
        <ArrowDown size={16} aria-hidden="true" />
      </button>
      <button type="button" className="icon-button" onClick={onHide} aria-label={t("hide", { name })} title={t("hide", { name })}>
        <EyeOff size={16} aria-hidden="true" />
      </button>
    </span>
  );
}

/**
 * The lists under the welcome text. The lists themselves are fixed by the server for about an hour;
 * which ones show, and in what order, is each person's own choice, kept in this browser under their
 * account (or as a visitor).
 */
export function HomeSections() {
  const t = useTranslations("home");
  const { user, isModerator } = useSession();
  const userId = user === undefined ? undefined : (user?.id ?? null);
  const [prefs, setPrefs] = useState<Prefs>(DEFAULT_PREFS);
  const [ready, setReady] = useState(false);
  const [editing, setEditing] = useState(false);
  const [dragging, setDragging] = useState<SectionKey>();
  const [data, setData] = useState<Home>();
  const [error, setError] = useState<unknown>();

  useEffect(() => {
    if (userId === undefined) return;
    setPrefs(loadPrefs(userId));
    setReady(true);
  }, [userId]);

  const load = useCallback(() => {
    setError(undefined);
    call(api.GET("/api/v1/home", { params: { query: {} } })).then(setData, setError);
  }, []);
  useEffect(load, [load]);

  const unpick = (id: string) =>
    call(api.DELETE("/api/v1/editors-picks/{id}", { params: { path: { id } } })).then(load, setError);

  const update = (next: Prefs) => {
    setPrefs(next);
    if (userId !== undefined) savePrefs(userId, next);
  };

  if (error !== undefined) return <ErrorState error={error} onRetry={load} />;
  if (!data || !ready) return <LoadingState />;

  const shown = visible(prefs);
  const sections = new Map(data.sections.map((s) => [s.key, s]));
  return (
    <section className="home-sections" aria-labelledby="home-discover">
      <div className="home-toolbar">
        <h2 id="home-discover">{t("discover")}</h2>
        <span className="home-toolbar-actions">
          {prefs.hidden.length > 0 && !editing && <span className="muted small">{t("hiddenCount", { count: prefs.hidden.length })}</span>}
          <button type="button" className="button" aria-pressed={editing} onClick={() => setEditing((e) => !e)}>
            <Settings2 size={16} aria-hidden="true" /> {editing ? t("done") : t("customize")}
          </button>
        </span>
      </div>
      {editing && <p className="muted small">{t("customizeHelp")}</p>}

      {shown.length === 0 && <p className="muted">{t("allHidden")}</p>}
      <div className="home-grid">
        {shown.map((k, i) => {
          const s = sections.get(k);
          const items = s?.items ?? [];
          return (
            <section
              key={k}
              className="home-section"
              aria-labelledby={`home-${k}`}
              draggable={editing}
              data-dragging={dragging === k ? "" : undefined}
              onDragStart={() => setDragging(k)}
              onDragEnd={() => setDragging(undefined)}
              onDragOver={(e) => editing && e.preventDefault()}
              onDrop={() => dragging && (update(moveTo(prefs, dragging, k)), setDragging(undefined))}
            >
              <div className="home-section-head">
                <h3 id={`home-${k}`}>{t(`sections.${k}.title`)}</h3>
                {editing && <Controls k={k} index={i} last={shown.length - 1} onMove={(d) => update(move(prefs, k, d))} onHide={() => update(hide(prefs, k))} />}
              </div>
              {items.length > 0 ? (
                <ol className="home-list">
                  {items.map((item) => (
                    <ItemRow key={item.id} section={k} item={item} onUnpick={k === "editors_picks" && isModerator ? () => unpick(item.id) : undefined} />
                  ))}
                </ol>
              ) : (
                <div className="home-empty">
                  <p>{t("nothing")}</p>
                  {k === "editors_picks" ? (
                    <p className="small">{isModerator ? t("picksHowTo") : t("picksSoon")}</p>
                  ) : (
                    <Link href={ADD_HREF[k]} className="button">
                      {t(`sections.${k}.add`)}
                    </Link>
                  )}
                </div>
              )}
            </section>
          );
        })}
      </div>

      {editing && (
        <div className="home-hidden" role="group" aria-label={t("hiddenTitle")}>
          <h3>{t("hiddenTitle")}</h3>
          {prefs.hidden.length === 0 ? (
            <p className="muted small">{t("noneHidden")}</p>
          ) : (
            <ul>
              {prefs.hidden.map((k) => (
                <li key={k}>
                  <span>{t(`sections.${k}.title`)}</span>
                  <button type="button" className="button" onClick={() => update(show(prefs, k))}>
                    <Eye size={16} aria-hidden="true" /> {t("showAgain", { name: t(`sections.${k}.title`) })}
                  </button>
                </li>
              ))}
            </ul>
          )}
          <button type="button" className="button ghost" onClick={() => update(DEFAULT_PREFS)}>
            <RotateCcw size={16} aria-hidden="true" /> {t("reset")}
          </button>
        </div>
      )}
      {data.generated_at && !editing && <p className="muted small">{t("updated", { when: new Date(data.generated_at) })}</p>}
    </section>
  );
}
