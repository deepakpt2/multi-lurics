"use client";

import { BookOpen, ExternalLink, Minus, Plus, X } from "lucide-react";
import { useLocale, useTranslations } from "next-intl";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { Link } from "@/i18n/navigation";
import { AddLyricsDialog } from "../contribute/add-lyrics";
import {
  DEFAULT_READING,
  SIZES,
  THEMES,
  TYPES,
  emptyPane,
  idsParam,
  initialPanes,
  isType,
  loadReading,
  ofType,
  saveReading,
  withType,
  type Lyric,
  type LyricType,
  type Pane,
  type Reading,
  type Theme,
} from "@/lib/lyric-panes";

function useNames() {
  const ui = useLocale();
  return useMemo(() => {
    const make = (type: "language" | "script") => {
      try {
        return new Intl.DisplayNames([ui], { type, fallback: "code" });
      } catch {
        return null;
      }
    };
    const lang = make("language");
    const script = make("script");
    return {
      /** "Malayalam"; a transliteration also says which script it is written in. */
      label: (l: Lyric) => {
        const name = lang?.of(l.language) ?? l.language;
        return l.type === "transliteration" ? `${name} · ${script?.of(l.script) ?? l.script}` : name;
      },
    };
  }, [ui]);
}

/**
 * A song's lyrics side by side. One column to begin with; "+" adds another, each with its own type
 * (original, translation, transliteration) and language, chosen from what exists for this song. On a
 * phone the columns are a row you swipe through. Each column can open on its own, full screen, and the
 * reading mode shows the same columns large and quiet.
 */
export function LyricPanes({
  lyrics,
  songPath,
  pageLocale,
  initial,
  fullScreen,
  songId,
  title,
}: {
  lyrics: Lyric[];
  /** The song page's path, e.g. `/song/kaatrin-mozhi`; the full-screen page lives under it. */
  songPath: string;
  pageLocale: string;
  /** Lyric ids to open with (the address of a full-screen view). */
  initial?: string[];
  fullScreen?: boolean;
  /** The song a lyric of a missing type is added to. */
  songId: string;
  title?: string;
}) {
  const t = useTranslations("public.panes");
  const tl = useTranslations("lyrics");
  const names = useNames();
  const [panes, setPanes] = useState<Pane[]>(() => initialPanes(lyrics, initial));
  const [reading, setReading] = useState(false);
  const [prefs, setPrefs] = useState<Reading>(DEFAULT_READING);
  const [at, setAt] = useState(0);
  const track = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const closer = useRef<HTMLButtonElement>(null);
  const id = useId();
  const [adding, setAdding] = useState<LyricType | null>(null);

  useEffect(() => setPrefs(loadReading()), []);
  // A full-screen view is addressable: the address always names the columns that are open.
  useEffect(() => {
    if (!fullScreen) return;
    const ids = idsParam(panes);
    const url = new URL(window.location.href);
    if (ids) url.searchParams.set("l", ids);
    else url.searchParams.delete("l");
    window.history.replaceState(null, "", url);
  }, [fullScreen, panes]);
  useEffect(() => {
    if (!reading) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setReading(false);
    document.addEventListener("keydown", onKey);
    document.body.classList.add("reading-open");
    closer.current?.focus();
    const back = opener.current;
    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.classList.remove("reading-open");
      back?.focus();
    };
  }, [reading]);

  const setPref = (p: Partial<Reading>) =>
    setPrefs((cur) => {
      const next = { ...cur, ...p };
      saveReading(next);
      return next;
    });
  const update = (key: string, f: (p: Pane) => Pane) => setPanes((ps) => ps.map((p) => (p.key === key ? f(p) : p)));
  const add = () => {
    const p = emptyPane(lyrics, panes);
    setPanes((ps) => [...ps, p]);
    // The new column slides into view (the next frame, once it exists).
    requestAnimationFrame(() => track.current?.querySelector(`[data-pane="${p.key}"]`)?.scrollIntoView?.({ behavior: "smooth", inline: "center", block: "nearest" }));
  };
  const onScroll = () => {
    const el = track.current;
    if (!el || el.clientWidth === 0) return;
    const first = el.querySelector<HTMLElement>("[data-pane]");
    const step = (first?.offsetWidth ?? el.clientWidth) + 12;
    setAt(Math.min(panes.length - 1, Math.max(0, Math.round(Math.abs(el.scrollLeft) / step))));
  };
  const labelOf = (l: Lyric, same: Lyric[]) => names.label(l) + (same.filter((x) => names.label(x) === names.label(l)).length > 1 ? ` (${same.indexOf(l) + 1})` : "");

  const body = (
    <div className="lyric-panes-wrap">
      {!reading && (
        <div className="lyric-panes-bar">
          <button
            type="button"
            className="button"
            onClick={(e) => {
              opener.current = e.currentTarget;
              setReading(true);
            }}
            aria-haspopup="dialog"
          >
            <BookOpen size={16} aria-hidden="true" /> {t("reading")}
          </button>
        </div>
      )}
      <div ref={track} className="lyric-panes" role="group" aria-label={t("label")} onScroll={onScroll} data-reading={reading || undefined}>
        {panes.map((p, n) => {
          const same = ofType(lyrics, p.type);
          const lyric = lyrics.find((l) => l.id === p.lyricId);
          return (
            <article key={p.key} className="lyric-pane" data-pane={p.key} aria-label={t("column", { n: n + 1 })}>
              <header className="lyric-pane-head">
                <label className="visually-hidden" htmlFor={`${id}-${p.key}-type`}>
                  {t("type")}
                </label>
                <select
                  id={`${id}-${p.key}-type`}
                  value={p.type}
                  onChange={(e) => isType(e.target.value) && update(p.key, (x) => withType(lyrics, x, e.target.value as LyricType))}
                >
                  {TYPES.map((x) => (
                    <option key={x} value={x}>
                      {tl(`types.${x}`)}
                    </option>
                  ))}
                </select>
                <label className="visually-hidden" htmlFor={`${id}-${p.key}-lang`}>
                  {t("language")}
                </label>
                <select
                  id={`${id}-${p.key}-lang`}
                  value={p.lyricId ?? ""}
                  disabled={same.length === 0}
                  onChange={(e) => update(p.key, (x) => ({ ...x, lyricId: e.target.value || null }))}
                >
                  <option value="">{t("choose")}</option>
                  {same.map((l) => (
                    <option key={l.id} value={l.id}>
                      {labelOf(l, same)}
                    </option>
                  ))}
                </select>
                <span className="lyric-pane-tools">
                  {lyric && !fullScreen && (
                    <Link
                      href={`${songPath}/lyrics?l=${lyric.id}`}
                      locale={pageLocale as never}
                      className="icon-button"
                      aria-label={t("fullScreen", { n: n + 1 })}
                      title={t("fullScreen", { n: n + 1 })}
                    >
                      <ExternalLink size={16} aria-hidden="true" />
                    </Link>
                  )}
                  {panes.length > 1 && (
                    <button type="button" className="icon-button" aria-label={t("close", { n: n + 1 })} title={t("close", { n: n + 1 })} onClick={() => setPanes((ps) => ps.filter((x) => x.key !== p.key))}>
                      <X size={16} aria-hidden="true" />
                    </button>
                  )}
                </span>
              </header>
              {lyric ? (
                <pre className="lyric-body" lang={lyric.language} dir={lyric.dir}>
                  {lyric.body}
                </pre>
              ) : same.length === 0 ? (
                <p className="muted lyric-pane-empty">
                  {t("none", { type: tl(`types.${p.type}`).toLowerCase() })}{" "}
                  <button type="button" className="link" onClick={() => setAdding(p.type)}>
                    {t("addOne")}
                  </button>
                </p>
              ) : (
                <p className="muted lyric-pane-empty">{t("pick")}</p>
              )}
            </article>
          );
        })}
        <button type="button" className="lyric-pane-add" onClick={add} aria-label={t("add")} title={t("add")}>
          <Plus size={22} aria-hidden="true" />
          <span>{t("add")}</span>
        </button>
      </div>
      {panes.length > 1 && (
        <p className="lyric-pane-position muted small" aria-live="polite">
          {t("position", { current: at + 1, total: panes.length })}
        </p>
      )}
    </div>
  );

  const dialog = adding && <AddLyricsDialog songId={songId} title={title} type={adding} onClose={() => setAdding(null)} />;
  if (!reading)
    return (
      <>
        {body}
        {dialog}
      </>
    );
  return (
    <div className="reading" role="dialog" aria-modal="true" aria-label={t("reading")} data-theme={prefs.theme} style={{ ["--reading-size" as string]: `${SIZES[prefs.size]}rem` }}>
      <div className="reading-bar">
        <span className="reading-controls" role="group" aria-label={t("textSize")}>
          <button type="button" className="icon-button" aria-label={t("smaller")} disabled={prefs.size === 0} onClick={() => setPref({ size: Math.max(0, prefs.size - 1) })}>
            <Minus size={16} aria-hidden="true" />
          </button>
          <button type="button" className="icon-button" aria-label={t("larger")} disabled={prefs.size === SIZES.length - 1} onClick={() => setPref({ size: Math.min(SIZES.length - 1, prefs.size + 1) })}>
            <Plus size={16} aria-hidden="true" />
          </button>
        </span>
        <span className="reading-controls" role="radiogroup" aria-label={t("theme")}>
          {THEMES.map((th: Theme) => (
            <button key={th} type="button" role="radio" aria-checked={prefs.theme === th} className="button reading-theme" data-theme-option={th} onClick={() => setPref({ theme: th })}>
              {t(`themes.${th}`)}
            </button>
          ))}
        </span>
        <button type="button" ref={closer} className="button" onClick={() => setReading(false)}>
          <X size={16} aria-hidden="true" /> {t("exitReading")}
        </button>
      </div>
      <div className="reading-body">{body}</div>
      {dialog}
    </div>
  );
}
