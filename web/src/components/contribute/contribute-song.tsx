"use client";

import { Plus, Trash2 } from "lucide-react";
import { useLocale, useTranslations } from "next-intl";
import { useEffect, useId, useState } from "react";
import { ApiProblem, api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Link } from "@/i18n/navigation";
import { buildSubmission, type Block, type Draft, type Prefill } from "@/lib/contribute";
import { DEVOTIONAL_FORMS } from "../catalogue/compositions";
import { useSubmit } from "../form";
import { languageName } from "../localization/localized";
import { LyricsDialog } from "./lyrics-dialog";
import { useSession } from "../session";
import { ProblemAlert } from "../states";
import { DEBOUNCE_MS } from "../entities/entity-picker";
import { NamePicker } from "./name-picker";

type Made = components["schemas"]["SongMade"];
const RELIGIONS = ["hindu", "sikh", "buddhist", "jain", "christian", "muslim", "other"] as const;
let n = 0;
const blockKey = () => `b${++n}`;

/** Existing songs with a similar title, as you type, so the same song is not added twice. */
function SimilarSongs({ title }: { title: string }) {
  const t = useTranslations("contribute");
  const q = title.trim();
  const [found, setFound] = useState<{ q: string; items: { id: string; text: string; detail?: string }[] } | null>(null);
  useEffect(() => {
    if (q.length < 2) return;
    const ctl = new AbortController();
    const timer = setTimeout(() => {
      call(api.GET("/api/v1/search", { params: { query: { q, type: "compositions", limit: 5 } }, signal: ctl.signal }))
        .then((r) => {
          if (ctl.signal.aborted) return;
          const items = (r.groups.find((g) => g.type === "compositions")?.items ?? []).map((i) => ({ id: i.id, text: i.display.text, detail: i.year ? String(i.year) : undefined }));
          setFound({ q, items });
        })
        .catch(() => undefined);
    }, DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      ctl.abort();
    };
  }, [q]);
  if (q.length < 2 || found?.q !== q || found.items.length === 0) return null;
  return (
    <div className="alert alert-warning similar" role="status">
      <p>{t("similar.title")}</p>
      <ul>
        {found.items.map((i) => (
          <li key={i.id}>
            <Link href={`/compositions/${i.id}`} target="_blank">
              <span dir="auto">{i.text}</span>
            </Link>
            {i.detail && <small className="muted"> {i.detail}</small>}
          </li>
        ))}
      </ul>
      <p className="muted small">{t("similar.hint")}</p>
    </div>
  );
}

export function ContributeSong({ prefill }: { prefill: Prefill }) {
  const t = useTranslations("contribute");
  const tl = useTranslations("lyrics");
  const tc = useTranslations("catalogue");
  const id = useId();
  const { user } = useSession();
  const locale = useLocale();
  const [editing, setEditing] = useState<Block | "new" | null>(null);
  const lock = (k: string) => prefill.locked.has(k);
  const [d, setD] = useState<Draft>(() => ({
    title: "",
    year: "",
    album: prefill.album,
    singers: prefill.singers,
    composers: prefill.composers,
    lyricists: prefill.lyricists,
    category: prefill.category ?? "mainstream",
    religion: prefill.religion ?? "",
    deity: prefill.deity,
    raga: prefill.raga,
    form: prefill.form ?? "",
    source: "",
    occasions: "",
    tags: prefill.tags,
    youtube: "",
    blocks: [],
  }));
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));
  const [done, setDone] = useState<Made | null>(null);
  const [duplicate, setDuplicate] = useState(false);
  const [missing, setMissing] = useState<string | null>(null);

  const s = useSubmit(async (allow: boolean) => {
    setDuplicate(false);
    const blocks = d.blocks.filter((b) => b.body.trim());
    const gap = !d.title.trim() ? "title" : blocks.length === 0 ? "lyrics" : null;
    if (gap) {
      setMissing(gap);
      return;
    }
    setMissing(null);
    try {
      setDone(await call(api.POST("/api/v1/contribute/song", { body: buildSubmission(d, allow) as never })));
    } catch (e) {
      if (e instanceof ApiProblem && e.code === "possible_duplicate") {
        setDuplicate(true);
        return;
      }
      throw e;
    }
  });

  if (user === undefined) return null;
  if (user === null)
    return (
      <p className="alert alert-warning">
        {t("signInFirst")} <Link href="/login">{t("signIn")}</Link>
      </p>
    );

  if (done) {
    const waiting = done.statuses.some((x) => x !== "published");
    return (
      <section className="contribute-done" role="status">
        <h2>{t("done.title")}</h2>
        <p>{waiting ? t("done.pending") : t("done.published")}</p>
        <p className="actions">
          <button type="button" className="button primary" onClick={() => {
              setDone(null);
              setD((x) => ({ ...x, title: "", year: "", blocks: [] }));
            }}>
            {t("done.another")}
          </button>
          <Link className="button" href="/">
            {t("done.home")}
          </Link>
        </p>
      </section>
    );
  }

  return (
    <>
        <form
          noValidate
          className="contribute-form"
          onSubmit={(e) => {
            e.preventDefault();
            void s.submit(false);
          }}
        >
          {s.error !== undefined && <ProblemAlert error={s.error} />}
          {missing && (
            <p className="alert alert-error" role="alert">
              {t(`missing.${missing as "title"}`)}
            </p>
          )}
          <fieldset className="contribute-section">
            <legend>{t("sections.about")}</legend>
            <div className="field">
              <label htmlFor={`${id}-title`}>{t("fields.title")}</label>
              <input id={`${id}-title`} dir="auto" value={d.title} onChange={(e) => set("title", e.target.value)} required />
              <p className="muted small">{t("hints.title")}</p>
            </div>
            <SimilarSongs title={d.title} />
            <NamePicker label={t("fields.album")} type="album" single value={d.album} onChange={(v) => set("album", v)} hint={lock("album") ? t("hints.fixed") : t("hints.album")} />
            <NamePicker label={t("fields.singers")} type="artist" value={d.singers} onChange={(v) => set("singers", v)} />
            <NamePicker label={t("fields.composers")} type="artist" value={d.composers} onChange={(v) => set("composers", v)} hint={t("hints.composers")} />
            <NamePicker label={t("fields.lyricists")} type="artist" value={d.lyricists} onChange={(v) => set("lyricists", v)} />
            <div className="field">
              <label htmlFor={`${id}-year`}>{t("fields.year")}</label>
              <input id={`${id}-year`} type="number" inputMode="numeric" value={d.year} onChange={(e) => set("year", e.target.value)} />
            </div>
            <div className="field">
              <label htmlFor={`${id}-cat`}>{t("fields.category")}</label>
              <select id={`${id}-cat`} value={d.category} disabled={lock("category")} onChange={(e) => set("category", e.target.value as Draft["category"])}>
                <option value="mainstream">{tc("songCategories.mainstream")}</option>
                <option value="devotional">{tc("songCategories.devotional")}</option>
              </select>
            </div>
            {d.category === "devotional" && (
              <>
                <div className="field">
                  <label htmlFor={`${id}-rel`}>{t("fields.religion")}</label>
                  <select id={`${id}-rel`} value={d.religion} disabled={lock("religion")} onChange={(e) => set("religion", e.target.value)}>
                    <option value="">{t("choose")}</option>
                    {RELIGIONS.map((r) => (
                      <option key={r} value={r}>
                        {tc(`religions.${r}`)}
                      </option>
                    ))}
                  </select>
                </div>
                <NamePicker label={t("fields.deity")} type="deity" single value={d.deity} onChange={(v) => set("deity", v)} />
                <div className="field">
                  <label htmlFor={`${id}-form`}>{t("fields.form")}</label>
                  <select id={`${id}-form`} value={d.form} disabled={lock("form")} onChange={(e) => set("form", e.target.value)}>
                    <option value="">{t("choose")}</option>
                    {DEVOTIONAL_FORMS.map((f) => (
                      <option key={f} value={f}>
                        {tc(`devotionalForms.${f}`)}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label htmlFor={`${id}-src`}>{t("fields.source")}</label>
                  <input id={`${id}-src`} dir="auto" value={d.source} onChange={(e) => set("source", e.target.value)} />
                </div>
                <div className="field">
                  <label htmlFor={`${id}-occ`}>{t("fields.occasions")}</label>
                  <input id={`${id}-occ`} dir="auto" value={d.occasions} onChange={(e) => set("occasions", e.target.value)} />
                  <p className="muted small">{t("hints.occasions")}</p>
                </div>
              </>
            )}
            <NamePicker label={t("fields.raga")} type="raga" single value={d.raga} onChange={(v) => set("raga", v)} />
            <NamePicker label={t("fields.tags")} type="tag" value={d.tags} onChange={(v) => set("tags", v)} />
            <div className="field">
              <label htmlFor={`${id}-yt`}>{t("fields.youtube")}</label>
              <input id={`${id}-yt`} dir="ltr" value={d.youtube} onChange={(e) => set("youtube", e.target.value)} />
              <p className="muted small">{t("hints.youtube")}</p>
            </div>
          </fieldset>

          <fieldset className="contribute-section">
            <legend>{t("sections.lyrics")}</legend>
            {d.blocks.length === 0 ? (
              <p className="muted">{t("lyricsEmpty")}</p>
            ) : (
              <ul className="lyric-entries">
                {d.blocks.map((b) => (
                  <li key={b.key} className="lyric-entry">
                    <div>
                      <strong>{tl(`types.${b.type}`)}</strong> <span className="muted">{languageName(b.language, locale)}</span>
                      <p className="lyric-preview" dir="auto" lang={b.language}>
                        {b.body.trim().split("\n").slice(0, 2).join(" / ")}
                      </p>
                    </div>
                    <span className="lyric-entry-tools">
                      <button type="button" className="button" onClick={() => setEditing(b)}>
                        {t("edit")}
                      </button>
                      <button type="button" className="button" onClick={() => set("blocks", d.blocks.filter((x) => x.key !== b.key))}>
                        <Trash2 size={16} aria-hidden="true" /> {t("removeBlock")}
                      </button>
                    </span>
                  </li>
                ))}
              </ul>
            )}
            <p>
              <button type="button" className="button primary" disabled={!d.title.trim()} onClick={() => setEditing("new")}>
                <Plus size={16} aria-hidden="true" /> {d.blocks.length === 0 ? t("addLyrics") : t("addMoreLyrics")}
              </button>
            </p>
            {!d.title.trim() && <p className="muted small">{t("hints.titleFirst")}</p>}
          </fieldset>

          {duplicate && (
            <div className="alert alert-warning" role="alert">
              <p>{t("duplicate")}</p>
              <p className="actions">
                <button type="button" className="button" onClick={() => void s.submit(true)}>
                  {t("duplicateAnyway")}
                </button>
              </p>
            </div>
          )}
          <p className="muted small">{t("reviewNote")}</p>
          <p className="actions">
            <button className="button primary" type="submit" disabled={s.submitting || !d.title.trim() || d.blocks.length === 0} aria-busy={s.submitting}>
              {s.submitting ? t("submitting") : t("submit")}
            </button>
          </p>
        </form>
        {editing && (
          <LyricsDialog
            title={d.title.trim()}
            initial={editing === "new" ? { language: prefill.language ?? "", type: d.blocks.some((x) => x.type === "original") ? "translation" : "original" } : editing}
            onClose={() => setEditing(null)}
            onSave={(b) => {
              set("blocks", editing === "new" ? [...d.blocks, { key: blockKey(), ...b }] : d.blocks.map((x) => (x.key === editing.key ? { ...x, ...b } : x)));
              setEditing(null);
            }}
          />
        )}
    </>
  );
}
