"use client";

import { Plus, Trash2 } from "lucide-react";
import { useTranslations } from "next-intl";
import { useEffect, useId, useState } from "react";
import { ApiProblem, api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Link } from "@/i18n/navigation";
import { buildSubmission, type Block, type Draft, type Prefill } from "@/lib/contribute";
import { DEVOTIONAL_FORMS } from "../catalogue/compositions";
import { Loaded, useLoad } from "../catalogue/common";
import { useSubmit } from "../form";
import { languageLabel, listLanguages, listScripts, scriptLabel } from "../localization/names";
import { useSession } from "../session";
import { ProblemAlert } from "../states";
import { DEBOUNCE_MS } from "../entities/entity-picker";
import { NamePicker } from "./name-picker";

type Made = components["schemas"]["SongMade"];
const TYPES = ["original", "translation", "transliteration"] as const;
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
  const refs = useLoad("refs", async () => ({ languages: (await listLanguages()).items, scripts: (await listScripts()).items }));
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
    blocks: [{ key: blockKey(), language: prefill.language ?? "", script: "", type: "original", body: "" }],
  }));
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));
  const setBlock = (key: string, patch: Partial<Block>) => set("blocks", d.blocks.map((b) => (b.key === key ? { ...b, ...patch } : b)));
  const [done, setDone] = useState<Made | null>(null);
  const [duplicate, setDuplicate] = useState(false);
  const [missing, setMissing] = useState<string | null>(null);

  const s = useSubmit(async (allow: boolean) => {
    setDuplicate(false);
    const blocks = d.blocks.filter((b) => b.body.trim());
    const gap = !d.title.trim() ? "title" : blocks.length === 0 ? "lyrics" : blocks.some((b) => !b.language) ? "language" : null;
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
              setD((x) => ({ ...x, title: "", year: "", blocks: [{ key: blockKey(), language: prefill.language ?? "", script: "", type: "original", body: "" }] }));
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
    <Loaded load={refs}>
      {({ languages, scripts }) => (
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

          {d.blocks.map((b, i) => {
            const rtl = languages.find((l) => l.iso639_3 === b.language)?.is_rtl ?? false;
            return (
              <fieldset key={b.key} className="contribute-section" data-block>
                <legend>{i === 0 ? t("sections.lyrics") : t("sections.another", { n: i + 1 })}</legend>
                <div className="field">
                  <label htmlFor={`${id}-${b.key}-lang`}>{t("fields.language")}</label>
                  <select
                    id={`${id}-${b.key}-lang`}
                    value={b.language}
                    onChange={(e) => setBlock(b.key, { language: e.target.value, script: languages.find((l) => l.iso639_3 === e.target.value)?.default_script ?? "" })}
                  >
                    <option value="">{t("choose")}</option>
                    {languages.map((l) => (
                      <option key={l.id} value={l.iso639_3}>
                        {languageLabel(l)}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label htmlFor={`${id}-${b.key}-script`}>{t("fields.script")}</label>
                  <select id={`${id}-${b.key}-script`} value={b.script} onChange={(e) => setBlock(b.key, { script: e.target.value })}>
                    <option value="">{t("scriptAuto")}</option>
                    {scripts.map((x) => (
                      <option key={x.id} value={x.iso15924}>
                        {scriptLabel(x)}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field" role="radiogroup" aria-label={t("fields.type")}>
                  <span className="field-label">{t("fields.type")}</span>
                  <div className="segmented">
                    {TYPES.map((x) => (
                      <button key={x} type="button" role="radio" aria-checked={b.type === x} onClick={() => setBlock(b.key, { type: x })}>
                        {tl(`types.${x}`)}
                      </button>
                    ))}
                  </div>
                  {b.type !== "original" && <p className="muted small">{t("hints.noOriginalNeeded")}</p>}
                </div>
                <div className="field">
                  <label htmlFor={`${id}-${b.key}-body`}>{t("fields.lyrics")}</label>
                  <textarea
                    id={`${id}-${b.key}-body`}
                    className="lyrics-input"
                    rows={14}
                    dir={rtl ? "rtl" : "ltr"}
                    lang={b.language || undefined}
                    translate="no"
                    value={b.body}
                    onChange={(e) => setBlock(b.key, { body: e.target.value })}
                  />
                  <p className="muted small">{tl("bodyHint")}</p>
                </div>
                {d.blocks.length > 1 && (
                  <button type="button" className="button" onClick={() => set("blocks", d.blocks.filter((x) => x.key !== b.key))}>
                    <Trash2 size={16} aria-hidden="true" /> {t("removeBlock")}
                  </button>
                )}
              </fieldset>
            );
          })}
          <p>
            <button
              type="button"
              className="button"
              onClick={() => set("blocks", [...d.blocks, { key: blockKey(), language: "", script: "", type: d.blocks.some((x) => x.type === "original") ? "translation" : "original", body: "" }])}
            >
              <Plus size={16} aria-hidden="true" /> {t("addAnother")}
            </button>
          </p>

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
            <button className="button primary" type="submit" disabled={s.submitting} aria-busy={s.submitting}>
              {s.submitting ? t("submitting") : t("submit")}
            </button>
          </p>
        </form>
      )}
    </Loaded>
  );
}
