"use client";

import { X } from "lucide-react";
import { useTranslations } from "next-intl";
import { useEffect, useId, useRef, useState } from "react";
import { api, call } from "@/api/client";
import { Link, useRouter } from "@/i18n/navigation";
import { withFilter, withoutFilter, type BrowseFilter, type BrowseState, type Suggestion } from "@/lib/browse";
import { Name } from "../localization/localized";
import { ProblemAlert } from "../states";
import { FACET_ICON } from "./facets";

export const DEBOUNCE_MS = 250;
export const MIN_CHARS = 2;

type Suggestions = { status: "idle" } | { status: "loading" } | { status: "done"; items: Suggestion[] } | { status: "error"; error: unknown };

/** A facet value's label: codes (religion, category) are translated, names are shown as served. */
export function useValueLabel() {
  const t = useTranslations();
  return (facet: string, label: BrowseFilter["label"]) => {
    if (facet === "religion" && t.has(`catalogue.religions.${label.text}`)) return t(`catalogue.religions.${label.text}` as never);
    if (facet === "category" && t.has(`browse.categories.${label.text}`)) return t(`browse.categories.${label.text}` as never);
    if (facet === "form" && t.has(`catalogue.devotionalForms.${label.text}`)) return t(`catalogue.devotionalForms.${label.text}` as never);
    return label.text;
  };
}

/**
 * The browse search: chips for the filters already chosen, and one input for the next. What it
 * suggests is limited to what exists together with the chips, so each step narrows the last.
 * Choosing a person, album or other metadata adds a chip; choosing a song opens it.
 */
export function FilterSearch({
  state,
  filters,
  onChange,
  busy,
}: {
  state: BrowseState;
  /** Labels for the chips, as the server resolved them. */
  filters: BrowseFilter[];
  onChange: (next: BrowseState) => void;
  busy?: boolean;
}) {
  const t = useTranslations("browse");
  const router = useRouter();
  const uid = useId();
  const input = useRef<HTMLInputElement>(null);
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const [retry, setRetry] = useState(0);
  const [found, setFound] = useState<Suggestions>({ status: "idle" });
  const valueLabel = useValueLabel();
  // Labels of filters just chosen, shown before the server has answered with its own.
  const known = useRef(new Map<string, BrowseFilter["label"]>());
  for (const f of filters) known.current.set(`${f.facet}:${f.slug}`, f.label);

  const q = text.trim();
  const enough = [...q].length >= MIN_CHARS;
  const filterKey = state.filters.map((f) => `${f.facet}=${f.slug}`).join("&");

  useEffect(() => {
    if (!enough) {
      setFound({ status: "idle" });
      return;
    }
    setFound({ status: "loading" });
    setActive(-1);
    const ctl = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const qs = new URLSearchParams([["q", q], ["limit", "10"], ...state.filters.map((f) => [f.facet, f.slug] as [string, string])]).toString();
        const r = await call(
          api.GET("/api/v1/browse/suggest", { params: { query: { q } }, querySerializer: () => qs, signal: ctl.signal }),
        );
        if (!ctl.signal.aborted) setFound({ status: "done", items: r.items });
      } catch (error) {
        if (!ctl.signal.aborted) setFound({ status: "error", error });
      }
    }, DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      ctl.abort();
    };
    // `filterKey` stands for state.filters: the suggestions are asked again whenever the chips change.
  }, [q, enough, filterKey, retry]); // eslint-disable-line react-hooks/exhaustive-deps

  const items = found.status === "done" ? found.items : [];
  const showList = open && enough;

  const choose = (s: Suggestion) => {
    void api.POST("/api/v1/search/suggest/accepted").catch(() => undefined);
    setText("");
    setOpen(false);
    if (s.kind === "song") {
      router.push(`/song/${encodeURIComponent(s.slug)}`, { locale: s.locale as never });
      return;
    }
    known.current.set(`${s.kind}:${s.slug}`, s.label);
    onChange(withFilter(state, { facet: s.kind, slug: s.slug }));
    input.current?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" && items.length) {
      e.preventDefault();
      setOpen(true);
      setActive((a) => (a + 1) % items.length);
    } else if (e.key === "ArrowUp" && items.length) {
      e.preventDefault();
      setOpen(true);
      setActive((a) => (a <= 0 ? items.length - 1 : a - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (showList && active >= 0 && items[active]) choose(items[active]);
      else if (q) router.push(`/search?q=${encodeURIComponent(q)}`);
    } else if (e.key === "Escape") {
      setOpen(false);
      setActive(-1);
    } else if (e.key === "Backspace" && text === "" && state.filters.length > 0) {
      onChange(withoutFilter(state, state.filters.length - 1));
    }
  };

  return (
    <div className="browse-search" role="search" aria-label={t("search.label")} aria-busy={busy || undefined}>
      <div className="filter-box" onClick={() => input.current?.focus()}>
        <ul className="filter-chips" aria-label={t("search.filters")}>
          {state.filters.map((f, i) => {
            const label = known.current.get(`${f.facet}:${f.slug}`);
            const Icon = FACET_ICON[f.facet];
            const unresolved = filters.find((x) => x.facet === f.facet && x.slug === f.slug)?.resolved === false;
            const name = label ? valueLabel(f.facet, label) : f.slug;
            return (
              <li key={`${f.facet}:${f.slug}`} className="filter-chip" data-facet={f.facet} data-unresolved={unresolved || undefined}>
                <Icon size={14} aria-hidden="true" />
                <span className="chip-kind">{t("search.kindLabel", { kind: t(`facet.${f.facet}`) })}</span>
                <bdi dir="auto" translate="no">
                  {name}
                </bdi>
                <button type="button" aria-label={t("search.remove", { facet: t(`facet.${f.facet}`), name })} onClick={() => onChange(withoutFilter(state, i))}>
                  <X size={14} aria-hidden="true" />
                </button>
              </li>
            );
          })}
        </ul>
        <input
          ref={input}
          id={`${uid}-input`}
          type="text"
          role="combobox"
          aria-label={t("search.label")}
          placeholder={state.filters.length ? t("search.placeholderMore") : t("search.placeholder")}
          aria-expanded={showList && items.length > 0}
          aria-controls={`${uid}-list`}
          aria-autocomplete="list"
          aria-activedescendant={active >= 0 ? `${uid}-o${active}` : undefined}
          autoComplete="off"
          autoCapitalize="off"
          spellCheck={false}
          dir="auto"
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onBlur={() => setOpen(false)}
          onKeyDown={onKeyDown}
        />
        {state.filters.length > 0 && (
          <button type="button" className="button ghost clear-all" onClick={() => onChange({ ...state, filters: [], page: 1 })}>
            {t("search.clearAll")}
          </button>
        )}
      </div>
      <div className="search-popover" hidden={!showList}>
        {found.status === "loading" && (
          <p className="muted small" role="status">
            {t("search.searching")}
          </p>
        )}
        {found.status === "error" && (
          <div onMouseDown={(e) => e.preventDefault()}>
            <ProblemAlert error={found.error} />
            <button type="button" className="button" onClick={() => setRetry((n) => n + 1)}>
              {t("search.retry")}
            </button>
          </div>
        )}
        <ul id={`${uid}-list`} role="listbox" aria-label={t("search.suggestions")} className="entity-options" hidden={items.length === 0}>
          {items.map((s, i) => {
            const Icon = s.kind === "song" ? null : FACET_ICON[s.kind];
            return (
              <li
                key={`${s.kind}:${s.slug}`}
                id={`${uid}-o${i}`}
                role="option"
                aria-selected={i === active}
                data-active={i === active || undefined}
                data-kind={s.kind}
                className="entity-option"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => choose(s)}
              >
                <span className="entity-title">
                  <small className="badge kind-badge" data-kind={s.kind}>
                    {Icon && <Icon size={12} aria-hidden="true" />}
                    {t(`facet.${s.kind}`)}
                  </small>
                  <Name display={{ ...s.label, text: valueLabel(s.kind, s.label) }} />
                </span>
                {s.via !== "name" && (
                  <span className="muted small">
                    {t("search.matched", { text: s.matched })}
                  </span>
                )}
                {s.kind === "song" && <span className="muted small">{t("search.opensSong")}</span>}
              </li>
            );
          })}
        </ul>
        {found.status === "done" && (
          <p className="muted small" role="status" onMouseDown={(e) => e.preventDefault()}>
            {items.length === 0 ? t("search.none", { query: q }) : t("search.count", { count: items.length })}{" "}
            <Link href={`/search?q=${encodeURIComponent(q)}`}>{t("search.searchAll", { query: q })}</Link>
          </p>
        )}
      </div>
    </div>
  );
}
