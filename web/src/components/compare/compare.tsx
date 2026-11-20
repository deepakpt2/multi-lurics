"use client";

import { useCallback, useEffect, useId, useMemo, useState } from "react";
import { useLocale, useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Picker, Verbatim } from "../catalogue/common";
import { EmptyState, LoadingState, ProblemAlert } from "../states";
import { languageName } from "../localization/localized";

type S = components["schemas"];
type Item = S["ContributionSummary"];
type Panel = S["ResolvedPanel"];

export const MAX_PANELS = 4;
const idsParam = (ids: string[]) => ids.join(",");

/** The URL carries contribution ids and nothing else; lyric text never goes into an address. */
export function parseIds(raw: string | null | undefined): string[] {
  return (raw ?? "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean)
    .slice(0, MAX_PANELS);
}

// ---------- one panel's picker: recording → type → language → script → exact contribution ----------

type Pick = { recording?: { id: string; label: string } };

/**
 * Chooses one published contribution. Each panel is independent, and the last step is the
 * contribution itself, so an approved alternative is picked as deliberately as a primary.
 */
export function PanelPicker({ onSelect, onCancel }: { onSelect: (contributionId: string) => void; onCancel?: () => void }) {
  const t = useTranslations("compare");
  const tl = useTranslations("lyrics");
  const locale = useLocale();
  const uid = useId();
  const [pick, setPick] = useState<Pick>({});
  const [items, setItems] = useState<Item[] | null>(null);
  const [error, setError] = useState<unknown>();
  const [kind, setKind] = useState("");
  const [language, setLanguage] = useState("");
  const [script, setScript] = useState("");

  const choose = async (o: { id: string; label: string }) => {
    setPick({ recording: o });
    setItems(null);
    setKind("");
    setLanguage("");
    setScript("");
    setError(undefined);
    try {
      const page = await call(api.GET("/api/v1/recordings/{id}/contributions", { params: { path: { id: o.id }, query: { limit: 100 } } }));
      setItems(page.items.filter((c) => c.status === "published"));
    } catch (e) {
      setError(e);
    }
  };

  const unique = (xs: string[]) => [...new Set(xs)].sort();
  const ofKind = useMemo(() => (items ?? []).filter((c) => !kind || c.type === kind), [items, kind]);
  const ofLang = useMemo(() => ofKind.filter((c) => !language || c.language === language), [ofKind, language]);
  const slot = useMemo(() => ofLang.filter((c) => !script || c.script === script), [ofLang, script]);

  return (
    <div className="panel-picker">
      <Picker
        label={t("recording")}
        search={async (prefix) => {
          const page = await call(api.GET("/api/v1/recordings", { params: { query: { prefix, limit: 8 } } }));
          return page.items.map((r) => ({ id: r.id, label: r.display.text || r.title }));
        }}
        onPick={choose}
      />
      {pick.recording && (
        <p className="muted small">
          {t("recordingChosen")} <Verbatim>{pick.recording.label}</Verbatim>
        </p>
      )}
      {error !== undefined && <ProblemAlert error={error} />}
      {pick.recording && items === null && error === undefined && <LoadingState />}
      {items && items.length === 0 && <EmptyState title={t("noPublished")} />}
      {items && items.length > 0 && (
        <div className="panel-fields">
          <div className="field">
            <label htmlFor={`${uid}-type`}>{t("type")}</label>
            <select id={`${uid}-type`} value={kind} onChange={(e) => { setKind(e.target.value); setLanguage(""); setScript(""); }}>
              <option value="">{t("any")}</option>
              {unique(items.map((c) => c.type)).map((k) => (
                <option key={k} value={k}>{tl(`types.${k as "original"}`)}</option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor={`${uid}-lang`}>{t("language")}</label>
            <select id={`${uid}-lang`} value={language} onChange={(e) => { setLanguage(e.target.value); setScript(""); }}>
              <option value="">{t("any")}</option>
              {unique(ofKind.map((c) => c.language)).map((l) => (
                <option key={l} value={l}>{languageName(l, locale)}</option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor={`${uid}-script`}>{t("script")}</label>
            <select id={`${uid}-script`} value={script} onChange={(e) => setScript(e.target.value)}>
              <option value="">{t("any")}</option>
              {unique(ofLang.map((c) => c.script)).map((s) => (
                <option key={s} value={s}>{s}</option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor={`${uid}-version`}>{t("version")}</label>
            <select
              id={`${uid}-version`}
              value=""
              onChange={(e) => e.target.value && onSelect(e.target.value)}
            >
              <option value="">{t("chooseVersion", { count: slot.length })}</option>
              {slot.map((c, i) => (
                <option key={c.id} value={c.id}>
                  {[
                    tl(`types.${c.type as "original"}`),
                    languageName(c.language, locale),
                    c.script,
                    c.is_primary ? t("primary") : t("alternative", { n: i + 1 }),
                  ].join(" · ")}
                </option>
              ))}
            </select>
          </div>
        </div>
      )}
      {onCancel && (
        <button type="button" className="button" onClick={onCancel}>
          {t("cancel")}
        </button>
      )}
    </div>
  );
}

// ---------- the comparison ----------

function PanelView({ panel, onRemove, onChange, index }: { panel: Panel; onRemove: () => void; onChange: () => void; index: number }) {
  const t = useTranslations("compare");
  const tl = useTranslations("lyrics");
  const locale = useLocale();
  const c = panel.content;
  return (
    <>
      {c ? (
        <>
          <header className="compare-head">
            <h2 dir="auto" translate="no">{c.composition_title}</h2>
            <p className="meta">
              <span className="badge">{tl(`types.${c.type as "original"}`)}</span>{" "}
              <span lang={c.language}>{languageName(c.language, locale)}</span>{tl("separator")}{c.script}{" "}
              <span className={c.is_primary ? "badge badge-primary" : "badge"}>{c.is_primary ? t("primary") : t("alternativeShort")}</span>
            </p>
            <p className="muted small" dir="auto">{c.recording_title}</p>
          </header>
          {/* The text's own language and direction, never the interface's. */}
          <pre className="lyrics" lang={c.language} dir={c.dir} translate="no" data-testid={`panel-body-${index}`}>
            {c.body}
          </pre>
        </>
      ) : (
        <EmptyState title={t("unavailable")} />
      )}
      <p className="actions">
        <button type="button" className="button" onClick={onChange}>{t("change")}</button>
        <button type="button" className="button" onClick={onRemove}>{t("remove")}</button>
      </p>
    </>
  );
}

export function Compare({ initialIds = [] }: { initialIds?: string[] }) {
  const t = useTranslations("compare");
  const [ids, setIds] = useState<string[]>(initialIds.slice(0, MAX_PANELS));
  const [adding, setAdding] = useState<number | null>(initialIds.length === 0 ? 0 : null);
  const [active, setActive] = useState(0);
  const [panels, setPanels] = useState<Panel[]>([]);
  const [state, setState] = useState<"idle" | "loading" | "error">("idle");
  const [error, setError] = useState<unknown>();
  const [saving, setSaving] = useState<{ name: string; done?: boolean; error?: unknown } | null>(null);
  const [copied, setCopied] = useState(false);
  const key = idsParam(ids);

  // Keep the address in step with the selection: ids only.
  useEffect(() => {
    try {
      const url = new URL(window.location.href);
      if (key) url.searchParams.set("c", key);
      else url.searchParams.delete("c");
      window.history.replaceState(null, "", url);
    } catch {
      /* the address is a convenience */
    }
  }, [key]);

  const load = useCallback(async () => {
    if (!key) {
      setPanels([]);
      return;
    }
    setState("loading");
    try {
      const res = await call(api.GET("/api/v1/comparisons/resolve", { params: { query: { ids: key } } }));
      setPanels(res.panels);
      setState("idle");
    } catch (e) {
      setError(e);
      setState("error");
    }
  }, [key]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (active >= ids.length) setActive(Math.max(0, ids.length - 1));
  }, [ids.length, active]);

  const set = (i: number, id: string) => {
    setIds((cur) => (i >= cur.length ? [...cur, id] : cur.map((x, j) => (j === i ? id : x))));
    setActive(i);
    setAdding(null);
  };

  const label = (i: number) => {
    const c = panels[i]?.content;
    return c ? `${i + 1}. ${c.language}·${c.script}${c.is_primary ? "" : "*"}` : t("panelN", { n: i + 1 });
  };

  const save = async () => {
    if (!saving) return setSaving({ name: "" });
    try {
      await call(api.POST("/api/v1/me/comparisons", { body: { name: saving.name, contribution_ids: ids } }));
      setSaving({ name: saving.name, done: true });
    } catch (e) {
      setSaving({ ...saving, error: e });
    }
  };

  return (
    <section className="compare-page">
      {ids.length > 0 && (
        <nav className="compare-tabs" aria-label={t("panels")}>
          {ids.map((_, i) => (
            <button key={i} type="button" className="button" aria-pressed={i === active} onClick={() => setActive(i)}>
              {label(i)}
            </button>
          ))}
        </nav>
      )}
      {state === "loading" && panels.length === 0 && <LoadingState />}
      {state === "error" && <ProblemAlert error={error} onReload={load} />}
      <div className="compare" data-panels={ids.length} style={{ "--panels": Math.max(1, ids.length) } as React.CSSProperties}>
        {ids.map((id, i) => {
          // Panels come back in request order; until they catch up with a changed selection, show a spinner.
          const panel = panels[i]?.id === id ? panels[i] : undefined;
          return (
            <article key={`${i}-${id}`} className="compare-panel" data-active={i === active ? "true" : "false"} aria-label={label(i)}>
              {adding === i ? (
                <PanelPicker onSelect={(cid) => set(i, cid)} onCancel={() => setAdding(null)} />
              ) : panel ? (
                <PanelView
                  panel={panel}
                  index={i}
                  onChange={() => setAdding(i)}
                  onRemove={() => setIds((cur) => cur.filter((_, j) => j !== i))}
                />
              ) : (
                <LoadingState />
              )}
            </article>
          );
        })}
        {adding === ids.length && ids.length < MAX_PANELS && (
          <article className="compare-panel" data-active="true">
            <PanelPicker onSelect={(cid) => set(ids.length, cid)} onCancel={ids.length ? () => setAdding(null) : undefined} />
          </article>
        )}
      </div>
      <p className="actions">
        {ids.length < MAX_PANELS && adding !== ids.length && (
          <button type="button" className="button primary" onClick={() => { setAdding(ids.length); setActive(ids.length); }}>
            {t("addPanel")}
          </button>
        )}
        {ids.length > 0 && (
          <>
            <button
              type="button"
              className="button"
              onClick={async () => {
                try {
                  await navigator.clipboard.writeText(window.location.href);
                  setCopied(true);
                } catch {
                  setCopied(false);
                }
              }}
            >
              {t("copyLink")}
            </button>
            <button type="button" className="button" onClick={save}>
              {t("save")}
            </button>
          </>
        )}
      </p>
      {copied && <p role="status" className="muted small">{t("copied")}</p>}
      {ids.length >= MAX_PANELS && <p className="muted small">{t("maxPanels", { max: MAX_PANELS })}</p>}
      {saving && (
        <form
          className="compare-save"
          onSubmit={async (e) => {
            e.preventDefault();
            await save();
          }}
        >
          {saving.done ? (
            <p role="status">{t("saved")}</p>
          ) : (
            <>
              <div className="field">
                <label htmlFor="compare-name">{t("name")}</label>
                <input id="compare-name" value={saving.name} maxLength={100} onChange={(e) => setSaving({ name: e.target.value })} required />
              </div>
              {saving.error !== undefined && <ProblemAlert error={saving.error} />}
              <button type="submit" className="button primary">{t("save")}</button>
            </>
          )}
        </form>
      )}
    </section>
  );
}
