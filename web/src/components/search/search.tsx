"use client";

import { useCallback, useEffect, useId, useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Link, useRouter } from "@/i18n/navigation";
import { Name } from "../localization/localized";
import { EmptyState, LoadingState, ProblemAlert } from "../states";

export type SearchItem = components["schemas"]["SearchItem"];
type Group = components["schemas"]["SearchGroup"];

export const DEBOUNCE_MS = 250;
/** Shorter queries are not sent: the server refuses them and they match too much to help. */
export const MIN_CHARS = 2;

export const entityHref = (i: Pick<SearchItem, "type" | "id">) =>
  ({ composition: "/compositions", recording: "/recordings", artist: "/artists", album: "/albums" })[i.type as string] + `/${i.id}`;

/** One result: where it goes, what it is, and — when it was not the shown name — what matched. */
function Result({ item, onChoose }: { item: SearchItem; onChoose?: () => void }) {
  const t = useTranslations("search");
  const tr = useTranslations();
  const kind =
    item.sub_kind && (item.type === "artist" || item.type === "album") ? tr(`catalogue.${item.type}Kinds.${item.sub_kind}`) : undefined;
  const via = item.match.via;
  return (
    <>
      <span className="entity-title">
        <Link href={entityHref(item)} onClick={onChoose}>
          <Name display={item.display} />
        </Link>
        <small className="badge">{t(`types.${item.type}`)}</small>
        {via !== "name" && <small className="badge">{t(`matched.${via}`)}</small>}
      </span>
      {via !== "name" && (
        <span className="muted small">
          {t("matchedText")} <bdi dir="auto" translate="no">{item.match.text}</bdi>
        </span>
      )}
      <span className="muted small">
        {[kind, item.detail, item.year, item.parent ? t("recordingOf", { title: item.parent.title }) : undefined]
          .filter((x) => x != null && x !== "")
          .join(t("separator"))}
      </span>
    </>
  );
}

type Typeahead = { status: "idle" } | { status: "loading" } | { status: "done"; items: SearchItem[] } | { status: "error"; error: unknown };

/**
 * Header typeahead. Waits for a pause in typing, cancels the request a newer keystroke makes
 * stale, and tells the server when a suggestion was chosen (a counter, nothing else).
 * Enter on a highlighted suggestion opens it; otherwise it runs the full search.
 */
export function SearchBox() {
  const t = useTranslations("search");
  const router = useRouter();
  const uid = useId();
  const [query, setQuery] = useState("");
  const [state, setState] = useState<Typeahead>({ status: "idle" });
  const [active, setActive] = useState(-1);
  const [retry, setRetry] = useState(0);
  const q = query.trim();
  const enough = [...q].length >= MIN_CHARS;

  useEffect(() => {
    if (!enough) {
      setState({ status: "idle" });
      return;
    }
    setState({ status: "loading" });
    setActive(-1);
    const ctl = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const r = await call(api.GET("/api/v1/search/suggest", { params: { query: { q, limit: 8 } }, signal: ctl.signal }));
        if (!ctl.signal.aborted) setState({ status: "done", items: r.items });
      } catch (error) {
        if (!ctl.signal.aborted) setState({ status: "error", error });
      }
    }, DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      ctl.abort();
    };
  }, [q, enough, retry]);

  const items = state.status === "done" ? state.items.filter((i) => i.type !== "recording") : [];
  const choose = (item: SearchItem) => {
    // Fire and forget: acceptance counting must never delay or block navigation.
    void api.POST("/api/v1/search/suggest/accepted").catch(() => undefined);
    setQuery("");
    router.push(entityHref(item));
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" && items.length) {
      e.preventDefault();
      setActive((a) => (a + 1) % items.length);
    } else if (e.key === "ArrowUp" && items.length) {
      e.preventDefault();
      setActive((a) => (a <= 0 ? items.length - 1 : a - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (active >= 0 && items[active]) choose(items[active]);
      else if (q) router.push(`/search?q=${encodeURIComponent(q)}`);
    } else if (e.key === "Escape") {
      setQuery("");
    }
  };
  const open = enough;

  return (
    <div className="search-box" role="search">
      <label className="visually-hidden" htmlFor={`${uid}-input`}>
        {t("label")}
      </label>
      <input
        id={`${uid}-input`}
        type="search"
        role="combobox"
        placeholder={t("placeholder")}
        aria-expanded={open && state.status === "done" && items.length > 0}
        aria-controls={`${uid}-list`}
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
      <div className="search-popover" hidden={!open}>
        {state.status === "loading" && (
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
        <ul id={`${uid}-list`} role="listbox" aria-label={t("results")} className="entity-options" hidden={items.length === 0}>
          {items.map((item, i) => (
            <li
              key={`${item.type}-${item.id}`}
              id={`${uid}-o${i}`}
              role="option"
              aria-selected={i === active}
              data-active={i === active || undefined}
              className="entity-option"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => choose(item)}
            >
              <Result item={item} />
            </li>
          ))}
        </ul>
        {state.status === "done" && (
          <p className="muted small" role="status">
            {items.length === 0 ? t("noResults", { query: q }) : t("resultCount", { count: items.length })}
            {" "}
            <Link href={`/search?q=${encodeURIComponent(q)}`}>{t("searchAll")}</Link>
          </p>
        )}
      </div>
    </div>
  );
}

type Results = { status: "loading" } | { status: "ready"; groups: Group[] } | { status: "error"; error: unknown };

/** The full search page: typed groups, each with its own "load more". */
export function SearchResults({ initialQuery }: { initialQuery: string }) {
  const t = useTranslations("search");
  const router = useRouter();
  const [input, setInput] = useState(initialQuery);
  const [state, setState] = useState<Results>({ status: "loading" });
  const [more, setMore] = useState<{ type: string; error?: unknown } | null>(null);
  const [nonce, setNonce] = useState(0);
  const q = initialQuery.trim();

  useEffect(() => {
    setInput(initialQuery);
  }, [initialQuery]);

  useEffect(() => {
    if (!q) return;
    setState({ status: "loading" });
    const ctl = new AbortController();
    call(api.GET("/api/v1/search", { params: { query: { q } }, signal: ctl.signal })).then(
      (r) => !ctl.signal.aborted && setState({ status: "ready", groups: r.groups }),
      (error) => !ctl.signal.aborted && setState({ status: "error", error }),
    );
    return () => ctl.abort();
  }, [q, nonce]);

  const loadMore = useCallback(
    async (g: Group) => {
      if (!g.next_cursor || more) return;
      setMore({ type: g.type });
      try {
        const r = await call(api.GET("/api/v1/search", { params: { query: { q, type: g.type as never, cursor: g.next_cursor } } }));
        const next = r.groups[0];
        setState((s) =>
          s.status !== "ready"
            ? s
            : { status: "ready", groups: s.groups.map((x) => (x.type === g.type ? { ...x, items: [...x.items, ...next.items], next_cursor: next.next_cursor } : x)) },
        );
        setMore(null);
      } catch (error) {
        setMore({ type: g.type, error });
      }
    },
    [q, more],
  );

  const shown = state.status === "ready" ? state.groups.filter((g) => g.type !== "recordings" && g.items.length > 0) : [];
  return (
    <>
      <form
        role="search"
        className="search-form"
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
          if (input.trim()) router.push(`/search?q=${encodeURIComponent(input.trim())}`);
        }}
      >
        <label htmlFor="search-q">{t("label")}</label>
        <input id="search-q" type="search" dir="auto" autoComplete="off" value={input} onChange={(e) => setInput(e.target.value)} />
        <button type="submit" className="button primary">
          {t("submit")}
        </button>
      </form>
      {!q && <EmptyState title={t("prompt")} />}
      {q && state.status === "loading" && <LoadingState />}
      {q && state.status === "error" && (
        <div>
          <ProblemAlert error={state.error} />
          <button type="button" className="button" onClick={() => setNonce((n) => n + 1)}>
            {t("retry")}
          </button>
        </div>
      )}
      {q && state.status === "ready" && shown.length === 0 && <EmptyState title={t("noResults", { query: q })} />}
      {shown.map((g) => (
        <section key={g.type} aria-labelledby={`group-${g.type}`}>
          <h2 id={`group-${g.type}`}>{t(`groups.${g.type as "compositions"}`)}</h2>
          <ul className="entity-list search-results">
            {g.items.map((item) => (
              <li key={`${item.type}-${item.id}`}>
                <Result item={item} />
              </li>
            ))}
          </ul>
          {more?.type === g.type && more.error !== undefined && <ProblemAlert error={more.error} />}
          {g.next_cursor && (
            <button type="button" className="button" disabled={more?.type === g.type && more.error === undefined} onClick={() => loadMore(g)}>
              {t("loadMore")}
            </button>
          )}
        </section>
      ))}
    </>
  );
}
