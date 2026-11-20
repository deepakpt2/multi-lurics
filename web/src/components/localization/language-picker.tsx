"use client";

import { useEffect, useId, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";

type Language = components["schemas"]["Language"];

const label = (l: Language) => `${l.endonym} (${l.name_en})`;
const search = (q: string) => call(api.GET("/api/v1/languages", { params: { query: { q, limit: 12 } } })).then((r) => r.items);

/**
 * Choose a language by typing: "sans" offers Sanskrit and the person picks it. Only a language chosen from
 * the suggestions counts; text that was not chosen is cleared when the box loses focus. `value` is the
 * language's code (ISO 639-3), `""` while nothing is chosen.
 */
export function LanguagePicker({
  id,
  value,
  onChange,
  onLoaded,
  inputRef,
  invalid,
}: {
  id?: string;
  value: string;
  /** Called with the chosen language, or `""` and no language once the typed text no longer is one. */
  onChange: (code: string, language?: Language) => void;
  /** Called when a language given in `value` has been looked up (to learn its direction and usual script). */
  onLoaded?: (language: Language) => void;
  inputRef?: React.Ref<HTMLInputElement>;
  invalid?: boolean;
}) {
  const t = useTranslations("languagePicker");
  const uid = useId();
  const inputId = id ?? `${uid}-in`;
  const listId = `${uid}-list`;
  const [text, setText] = useState("");
  const [chosen, setChosen] = useState<string>("");
  const [items, setItems] = useState<Language[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [asked, setAsked] = useState(false);
  const stamp = useRef(0);

  // A language given from outside (a link, a saved entry) is shown by its name.
  useEffect(() => {
    if (!value || value === chosen) return;
    let live = true;
    void search(value)
      .then((r) => {
        const l = r.find((x) => x.iso639_3 === value || x.iso639_1 === value);
        if (live && l) {
          setText(label(l));
          setChosen(l.iso639_3);
          onLoaded?.(l);
        }
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);
  useEffect(() => {
    if (!value) {
      setChosen("");
      setText("");
    }
  }, [value]);

  // Suggestions follow the typing, a moment after the last key.
  useEffect(() => {
    if (!open) return;
    const mine = ++stamp.current;
    const h = setTimeout(() => {
      search(chosen ? "" : text.trim())
        .then((r) => {
          if (stamp.current !== mine) return;
          setItems(r);
          setActive(0);
          setAsked(true);
        })
        .catch(() => stamp.current === mine && (setItems([]), setAsked(true)));
    }, 120);
    return () => clearTimeout(h);
  }, [text, open, chosen]);

  const pick = (l: Language) => {
    setText(label(l));
    setChosen(l.iso639_3);
    setOpen(false);
    onChange(l.iso639_3, l);
  };

  const onKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setOpen(true);
      setActive((a) => Math.min(a + 1, Math.max(items.length - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter" && open && items[active]) {
      e.preventDefault();
      e.stopPropagation();
      pick(items[active]);
    } else if (e.key === "Escape" && open) {
      e.stopPropagation();
      setOpen(false);
    }
  };

  return (
    <div className="language-picker">
      <input
        id={inputId}
        ref={inputRef}
        type="text"
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={open && items[active] ? `${listId}-${active}` : undefined}
        aria-invalid={invalid ? true : undefined}
        autoComplete="off"
        autoCapitalize="off"
        spellCheck={false}
        placeholder={t("placeholder")}
        value={text}
        onFocus={() => setOpen(true)}
        onChange={(e) => {
          setText(e.target.value);
          setOpen(true);
          if (chosen) {
            setChosen("");
            onChange("");
          }
        }}
        onKeyDown={onKey}
        // A choice by mouse happens before this runs (the options keep focus with mousedown).
        onBlur={() => {
          setOpen(false);
          if (!chosen) setText("");
        }}
      />
      {open && (
        <ul id={listId} role="listbox" className="suggest-list language-list">
          {items.map((l, i) => (
            <li
              key={l.id}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === active}
              className={i === active ? "active" : undefined}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => pick(l)}
            >
              <span lang={l.iso639_1 ?? l.iso639_3}>{l.endonym}</span> <span className="muted">{l.name_en}</span>
            </li>
          ))}
          {asked && items.length === 0 && (
            <li role="option" aria-disabled="true" aria-selected="false" className="muted">
              {t("none")}
            </li>
          )}
        </ul>
      )}
      <p className="muted small">{t("hint")}</p>
    </div>
  );
}
