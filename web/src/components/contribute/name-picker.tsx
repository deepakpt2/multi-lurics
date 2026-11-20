"use client";

import { X } from "lucide-react";
import { useTranslations } from "next-intl";
import { useEffect, useId, useState } from "react";
import { DEBOUNCE_MS, suggest, type EntityType, type Suggestion } from "../entities/entity-picker";

/** A person, album, raga, deity or tag: an existing one (its `id`) or a new name typed in. */
export type Chosen = { id?: string; name: string; locked?: boolean };

/**
 * A text box that suggests what already exists as you type, so people choose it instead of making a
 * second copy. Typing a name that is not there and pressing Enter (or the "new" row) adds it as new;
 * the server still checks names and aliases again when the song is saved. `locked` entries (pre-filled
 * from the page the contributor came from) are shown but cannot be removed.
 */
export function NamePicker({
  label,
  type,
  value,
  onChange,
  single,
  hint,
}: {
  label: string;
  type: Extract<EntityType, "artist" | "album" | "raga" | "deity" | "tag">;
  value: Chosen[];
  onChange: (v: Chosen[]) => void;
  /** At most one value (an album, a raga). */
  single?: boolean;
  hint?: string;
}) {
  const t = useTranslations("contribute.picker");
  const uid = useId();
  const [text, setText] = useState("");
  const [items, setItems] = useState<Suggestion[]>([]);
  const [active, setActive] = useState(-1);
  const q = text.trim();
  const full = single && value.length > 0;

  useEffect(() => {
    if (!q || full) {
      setItems([]);
      return;
    }
    const ctl = new AbortController();
    const timer = setTimeout(() => {
      suggest(type, q, ctl.signal)
        .then((r) => {
          if (ctl.signal.aborted) return;
          setItems(r);
          setActive(-1);
        })
        .catch(() => undefined);
    }, DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      ctl.abort();
    };
  }, [q, type, full]);

  const taken = (name: string, id?: string) => value.some((v) => (id && v.id === id) || v.name.toLowerCase() === name.toLowerCase());
  const add = (c: Chosen) => {
    if (!c.name.trim() || taken(c.name, c.id)) return setText("");
    onChange(single ? [c] : [...value, c]);
    setText("");
    setItems([]);
  };
  const exact = items.some((s) => s.display.text.toLowerCase() === q.toLowerCase());
  const rows = [
    ...items.filter((s) => !taken(s.display.text, s.id)).map((s) => ({ key: s.id, label: s.display.text, note: s.detail ?? undefined, pick: () => add({ id: s.id, name: s.display.text }) })),
    ...(q && !exact && !taken(q) ? [{ key: "new", label: t("addNew", { name: q }), note: undefined, pick: () => add({ name: q }) }] : []),
  ];
  const listId = `${uid}-list`;

  return (
    <div className="field name-picker">
      <label htmlFor={`${uid}-in`}>{label}</label>
      {value.length > 0 && (
        <ul className="chips">
          {value.map((v) => (
            <li key={`${v.id ?? ""}-${v.name}`} className="chip" data-new={!v.id || undefined}>
              <span dir="auto">{v.name}</span>
              {!v.id && <small className="muted">{t("new")}</small>}
              {v.locked ? (
                <small className="muted">{t("fixed")}</small>
              ) : (
                <button type="button" className="chip-remove" aria-label={t("remove", { name: v.name })} onClick={() => onChange(value.filter((x) => x !== v))}>
                  <X size={14} aria-hidden="true" />
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {!full && (
        <>
          <input
            id={`${uid}-in`}
            type="text"
            dir="auto"
            autoComplete="off"
            role="combobox"
            aria-expanded={rows.length > 0}
            aria-controls={listId}
            aria-activedescendant={active >= 0 ? `${uid}-o${active}` : undefined}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setActive((a) => Math.min(rows.length - 1, a + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActive((a) => Math.max(-1, a - 1));
              } else if (e.key === "Enter" && q) {
                e.preventDefault();
                (rows[active] ?? rows.find((r) => r.key !== "new") ?? rows[0])?.pick();
              }
            }}
          />
          {rows.length > 0 && (
            <ul id={listId} role="listbox" className="suggest-list" aria-label={t("suggestions")}>
              {rows.map((r, i) => (
                <li key={r.key} id={`${uid}-o${i}`} role="option" aria-selected={i === active}>
                  <button type="button" tabIndex={-1} data-active={i === active || undefined} onClick={r.pick}>
                    <span dir="auto">{r.label}</span>
                    {r.note && <small className="muted"> {r.note}</small>}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
      {hint && <p className="muted small">{hint}</p>}
    </div>
  );
}
