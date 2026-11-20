"use client";

import { X } from "lucide-react";
import { useTranslations } from "next-intl";
import { useEffect, useId, useRef, useState } from "react";
import { ENGLISH, type Block } from "@/lib/contribute";
import { useLoad } from "../catalogue/common";
import type { components } from "@/api/schema";
import { LanguagePicker } from "../localization/language-picker";
import { languageLabel, listScripts, scriptLabel } from "../localization/names";
import { ProblemAlert } from "../states";

const TYPES = ["original", "translation", "transliteration"] as const;

/**
 * The window for one set of lyrics: what it is (original, translation, transliteration), its language,
 * then the text. Saving hands it to the page that opened the window and closes it. Nothing here knows
 * about recordings or compositions.
 */
export function LyricsDialog({
  title,
  initial,
  onClose,
  onSave,
}: {
  /** The song's name, shown in the heading. */
  title?: string;
  initial?: Partial<Block>;
  onClose: () => void;
  /** Resolves when saved; a rejection is shown in the window and the text is kept. */
  onSave: (b: Pick<Block, "type" | "language" | "script" | "body" | "title">) => Promise<void> | void;
}) {
  const t = useTranslations("contribute.dialog");
  const tl = useTranslations("lyrics");
  const tc = useTranslations("contribute");
  const id = useId();
  const refs = useLoad("scripts", async () => (await listScripts()).items);
  const [chosen, setChosen] = useState<components["schemas"]["Language"] | null>(null);
  const [type, setType] = useState<Block["type"]>(initial?.type ?? "original");
  const [language, setLanguage] = useState(initial?.language ?? "");
  const [script, setScript] = useState(initial?.script ?? "");
  const [body, setBody] = useState(initial?.body ?? "");
  const [songTitle, setSongTitle] = useState(initial?.title ?? "");
  const [error, setError] = useState<unknown>(undefined);
  const [gap, setGap] = useState<"language" | "title" | "lyrics" | null>(null);
  const [busy, setBusy] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const first = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    document.body.classList.add("reading-open");
    first.current?.focus();
    return () => {
      document.body.classList.remove("reading-open");
      opener?.focus?.();
    };
  }, []);

  const scripts = refs.state.status === "ready" ? refs.state.data : [];
  const rtl = chosen?.is_rtl ?? false;

  const save = async () => {
    const g = !language ? "language" : language !== ENGLISH && !songTitle.trim() ? "title" : !body.trim() ? "lyrics" : null;
    setGap(g);
    if (g) return;
    setBusy(true);
    setError(undefined);
    try {
      await onSave({ type, language, script, body, title: language === ENGLISH ? "" : songTitle.trim() });
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  // Keep Tab inside the window and close on Escape.
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
    } else if (e.key === "Tab" && box.current) {
      const items = [...box.current.querySelectorAll<HTMLElement>("button, select, textarea, input, a[href]")].filter((x) => !x.hasAttribute("disabled"));
      const [a, z] = [items[0], items[items.length - 1]];
      if (e.shiftKey && document.activeElement === a) {
        e.preventDefault();
        z?.focus();
      } else if (!e.shiftKey && document.activeElement === z) {
        e.preventDefault();
        a?.focus();
      }
    }
  };

  return (
    <div className="modal-backdrop" onKeyDown={onKeyDown}>
      <div ref={box} className="modal" role="dialog" aria-modal="true" aria-labelledby={`${id}-h`}>
        <header className="modal-head">
          <h2 id={`${id}-h`}>{title ? t("titleFor", { title }) : t("title")}</h2>
          <button type="button" className="icon-button" aria-label={t("close")} onClick={onClose}>
            <X size={16} aria-hidden="true" />
          </button>
        </header>
        {error !== undefined && <ProblemAlert error={error} />}
        {gap && (
          <p className="alert alert-error" role="alert">
            {t(`missing.${gap}`)}
          </p>
        )}
        <div className="field" role="radiogroup" aria-label={tc("fields.type")}>
          <span className="field-label">{tc("fields.type")}</span>
          <div className="segmented">
            {TYPES.map((x) => (
              <button key={x} type="button" role="radio" aria-checked={type === x} onClick={() => setType(x)}>
                {tl(`types.${x}`)}
              </button>
            ))}
          </div>
          {type !== "original" && <p className="muted small">{tc("hints.noOriginalNeeded")}</p>}
        </div>
        <div className="field">
          <label htmlFor={`${id}-lang`}>{tc("fields.language")}</label>
          <LanguagePicker
            id={`${id}-lang`}
            inputRef={first}
            value={language}
            onLoaded={setChosen}
            onChange={(code, l) => {
              setLanguage(code);
              setChosen(l ?? null);
              setScript(l?.default_script ?? "");
            }}
          />
        </div>
        {language && (
          <>
            <div className="field">
              <label htmlFor={`${id}-script`}>{tc("fields.script")}</label>
              <select id={`${id}-script`} value={script} onChange={(e) => setScript(e.target.value)}>
                <option value="">{tc("scriptAuto")}</option>
                {scripts.map((x) => (
                  <option key={x.id} value={x.iso15924}>
                    {scriptLabel(x)}
                  </option>
                ))}
              </select>
            </div>
            {language !== ENGLISH && (
              <div className="field">
                <label htmlFor={`${id}-title`}>{t("titleIn", { language: chosen ? languageLabel(chosen) : language })}</label>
                <input id={`${id}-title`} dir={rtl ? "rtl" : "auto"} lang={language} translate="no" value={songTitle} onChange={(e) => setSongTitle(e.target.value)} />
                <p className="muted small">{t("titleInHint")}</p>
              </div>
            )}
            <div className="field">
              <label htmlFor={`${id}-body`}>{tc("fields.lyrics")}</label>
              <textarea id={`${id}-body`} className="lyrics-input" rows={14} dir={rtl ? "rtl" : "ltr"} lang={language} translate="no" value={body} onChange={(e) => setBody(e.target.value)} />
              <p className="muted small">{tl("bodyHint")}</p>
            </div>
          </>
        )}
        <p className="actions">
          <button type="button" className="button primary" disabled={busy} aria-busy={busy} onClick={() => void save()}>
            {busy ? tc("submitting") : t("save")}
          </button>
          <button type="button" className="button" onClick={onClose}>
            {t("cancel")}
          </button>
        </p>
      </div>
    </div>
  );
}
