"use client";

import { useEffect, useId, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { ApiProblem, api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Name } from "../localization/localized";
import { ProblemAlert } from "../states";

export type Suggestion = components["schemas"]["EntitySuggestion"];
export type EntityType = Suggestion["type"] extends string ? "artist" | "album" | "label" | "publisher" | "genre" | "raga" | "deity" | "tag" | "language" | "script" : never;
export type Picked = { id: string; label: string; note?: string };

/** Types a person may propose; languages and scripts are governed reference data. */
const PROPOSABLE: readonly EntityType[] = ["artist", "album", "label", "publisher", "genre", "raga", "deity", "tag"];
export const DEBOUNCE_MS = 250;

export const suggest = (type: EntityType, q: string, signal?: AbortSignal) =>
  call(api.GET("/api/v1/entities/suggest", { params: { query: { type, q, limit: 8 } }, signal })).then((r) => r.items);

type State = { status: "idle" } | { status: "loading" } | { status: "done"; items: Suggestion[] } | { status: "error"; error: unknown };

/**
 * The one way to choose an existing artist, album, label, publisher, genre, language or script.
 * A selection is an id, never typed text. Results come from the shared server-side search
 * (case, spacing, punctuation and diacritics do not matter) after a short pause; each new
 * keystroke cancels the previous request.
 */
export function EntityPicker({
  type,
  label,
  onPick,
  excludeIds,
}: {
  type: EntityType;
  label: string;
  onPick: (picked: Picked) => void;
  excludeIds?: readonly string[];
}) {
  const t = useTranslations("entities");
  const tr = useTranslations();
  const uid = useId();
  const listId = `${uid}-list`;
  const [query, setQuery] = useState("");
  const [state, setState] = useState<State>({ status: "idle" });
  const [active, setActive] = useState(-1);
  const [retry, setRetry] = useState(0);
  const [proposing, setProposing] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const q = query.trim();

  useEffect(() => {
    if (!q) {
      setState({ status: "idle" });
      return;
    }
    setState({ status: "loading" });
    setActive(-1);
    const ctl = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const items = await suggest(type, q, ctl.signal);
        if (!ctl.signal.aborted) setState({ status: "done", items });
      } catch (error) {
        if (!ctl.signal.aborted) setState({ status: "error", error });
      }
    }, DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      ctl.abort();
    };
  }, [q, type, retry]);

  const items = state.status === "done" ? state.items.filter((s) => !excludeIds?.includes(s.id)) : [];
  const pick = (s: Suggestion) => {
    onPick({ id: s.id, label: s.display.text, note: s.detail ?? undefined });
    setQuery("");
    setProposing(false);
    input.current?.focus();
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" && items.length) {
      e.preventDefault();
      setActive((a) => (a + 1) % items.length);
    } else if (e.key === "ArrowUp" && items.length) {
      e.preventDefault();
      setActive((a) => (a <= 0 ? items.length - 1 : a - 1));
    } else if (e.key === "Enter") {
      // Never submit the surrounding form from the picker.
      e.preventDefault();
      if (active >= 0 && items[active]) pick(items[active]);
    } else if (e.key === "Escape") {
      setQuery("");
    }
  };
  const kindLabel = (s: Suggestion) =>
    s.sub_kind && (s.type === "artist" || s.type === "album") ? tr(`catalogue.${s.type}Kinds.${s.sub_kind}`) : undefined;
  const open = q !== "";

  return (
    <div className="entity-picker">
      <label htmlFor={`${uid}-input`}>{label}</label>
      <input
        ref={input}
        id={`${uid}-input`}
        type="text"
        role="combobox"
        aria-expanded={open && state.status === "done" && items.length > 0}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={active >= 0 ? `${uid}-o${active}` : undefined}
        autoComplete="off"
        autoCapitalize="off"
        spellCheck={false}
        dir="auto"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={onKeyDown}
      />
      {open && state.status === "loading" && (
        <p className="muted small" role="status">
          {t("searching")}
        </p>
      )}
      {state.status === "error" && (
        <div>
          <ProblemAlert error={state.error} />
          <button type="button" className="button" onClick={() => setRetry((n) => n + 1)}>
            {t("retry")}
          </button>
        </div>
      )}
      <ul id={listId} role="listbox" aria-label={t("results")} className="entity-options" hidden={!(open && items.length > 0)}>
        {items.map((s, i) => (
          <li
            key={s.id}
            id={`${uid}-o${i}`}
            role="option"
            aria-selected={i === active}
            data-active={i === active || undefined}
            className="entity-option"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => pick(s)}
          >
            <span className="entity-title">
              <Name display={s.display} />
              {s.matched_via !== "name" && <small className="badge">{t(`matched.${s.matched_via}`)}</small>}
            </span>
            <span className="muted small">
              {[kindLabel(s), s.detail, s.year, s.code].filter((x) => x != null && x !== "").join(t("separator"))}
            </span>
            {s.localized_names.length > 0 && (
              <span className="muted small">
                {s.localized_names.map((n, j) => (
                  <span key={`${n.language}-${j}`} lang={n.language} dir="auto" translate="no">
                    {j > 0 && t("separator")}
                    {n.name}
                  </span>
                ))}
              </span>
            )}
            {s.related.length > 0 && (
              <span className="muted small">
                {t("related", { titles: s.related.map((r) => r.title).join(t("separator")) })}
              </span>
            )}
          </li>
        ))}
      </ul>
      {state.status === "done" && (
        <p className="muted small" role="status">
          {items.length === 0 ? t("noResults", { query: q }) : t("resultCount", { count: items.length })}
        </p>
      )}
      {state.status === "done" && items.length === 0 && PROPOSABLE.includes(type) && !proposing && (
        <button type="button" className="button" onClick={() => setProposing(true)}>
          {t("propose", { type: t(`types.${type}`) })}
        </button>
      )}
      {proposing && <ProposeEntity type={type} initialName={q} onUse={pick} onClose={() => setProposing(false)} />}
    </div>
  );
}

type Step = { name: "form" } | { name: "likely"; items: Suggestion[] } | { name: "sent" };

/**
 * Proposes a new entity for moderator review. The likely matches are shown first; a proposal is
 * only queued once the person confirms none of them is the one they mean. Not a <form>: it lives
 * inside other forms.
 */
export function ProposeEntity({
  type,
  initialName,
  onUse,
  onClose,
}: {
  type: EntityType;
  initialName: string;
  onUse: (s: Suggestion) => void;
  onClose: () => void;
}) {
  const t = useTranslations("entities");
  const tc = useTranslations("catalogue");
  const [name, setName] = useState(initialName);
  const [kind, setKind] = useState(type === "album" ? "album" : "person");
  const [disambiguation, setDisambiguation] = useState("");
  const [step, setStep] = useState<Step>({ name: "form" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>();
  const kinds = type === "album" ? (["album", "single", "ep", "compilation", "soundtrack", "other"] as const) : (["person", "group", "other"] as const);

  const send = async (confirm: boolean) => {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      await call(
        api.POST("/api/v1/entities/proposals", {
          body: {
            entity_type: type,
            name,
            kind: type === "artist" || type === "album" ? kind : null,
            disambiguation: type === "artist" && disambiguation.trim() ? disambiguation.trim() : null,
            confirm_distinct: confirm,
          },
        }),
      );
      setStep({ name: "sent" });
    } catch (e) {
      if (e instanceof ApiProblem && e.code === "likely_duplicates") {
        try {
          setStep({ name: "likely", items: await suggest(type, name) });
        } catch (inner) {
          setError(inner);
        }
      } else {
        setError(e);
      }
    } finally {
      setBusy(false);
    }
  };

  if (step.name === "sent") {
    return (
      <div className="alert" role="status">
        <p>{t("sent")}</p>
        <button type="button" className="button" onClick={onClose}>
          {t("close")}
        </button>
      </div>
    );
  }
  return (
    <div className="propose" role="group" aria-label={t("proposeTitle", { type: t(`types.${type}`) })}>
      {error !== undefined && <ProblemAlert error={error} />}
      {step.name === "likely" ? (
        <>
          <p>
            <strong>{t("likelyTitle")}</strong> {t("likelyBody")}
          </p>
          <ul className="entity-options">
            {step.items.map((s) => (
              <li key={s.id} className="entity-option">
                <span className="entity-title">
                  <Name display={s.display} />
                </span>
                <button type="button" className="button" onClick={() => onUse(s)}>
                  {t("useThis")}
                </button>
              </li>
            ))}
          </ul>
          <button type="button" className="button" disabled={busy} onClick={() => send(true)}>
            {t("noneOfThese")}
          </button>
        </>
      ) : (
        <>
          <div className="field">
            <label htmlFor={`${type}-p-name`}>{t("proposeName")}</label>
            <input id={`${type}-p-name`} dir="auto" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && e.preventDefault()} />
          </div>
          {(type === "artist" || type === "album") && (
            <div className="field">
              <label htmlFor={`${type}-p-kind`}>{t("proposeKind")}</label>
              <select id={`${type}-p-kind`} value={kind} onChange={(e) => setKind(e.target.value)}>
                {kinds.map((k) => (
                  <option key={k} value={k}>
                    {tc(`${type}Kinds.${k}`)}
                  </option>
                ))}
              </select>
            </div>
          )}
          {type === "artist" && (
            <div className="field">
              <label htmlFor="artist-p-dis">{t("proposeDisambiguation")}</label>
              <input id="artist-p-dis" dir="auto" value={disambiguation} onChange={(e) => setDisambiguation(e.target.value)} onKeyDown={(e) => e.key === "Enter" && e.preventDefault()} />
            </div>
          )}
          <p className="actions">
            <button type="button" className="button primary" disabled={busy || !name.trim()} aria-busy={busy} onClick={() => send(false)}>
              {t("submitProposal")}
            </button>
            <button type="button" className="button" onClick={onClose}>
              {t("cancel")}
            </button>
          </p>
        </>
      )}
    </div>
  );
}
