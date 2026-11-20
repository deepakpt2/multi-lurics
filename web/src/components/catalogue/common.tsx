"use client";

import { EntityPicker, type EntityType } from "../entities/entity-picker";
import { useEffect, useId, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { ApiProblem, type Problem } from "@/api/client";
import type { components } from "@/api/schema";
import { Link } from "@/i18n/navigation";
import { Name } from "../localization/localized";
import { useFieldError } from "../form";
import { EmptyState, ErrorState, LoadingState, ProblemAlert } from "../states";

export type S = components["schemas"];
export type Kind = "artists" | "compositions" | "recordings" | "albums";
type FieldErrors = { field: string; code: string }[];

export const CREDIT_ROLES = {
  compositions: ["composer", "lyricist"],
  recordings: ["performer", "featured_performer", "arranger", "producer", "conductor"],
  albums: ["primary_artist", "producer"],
} as const satisfies Record<string, readonly S["CreditRole"][]>;
export const RELATIONSHIP_KINDS = [
  "cover",
  "remake",
  "live",
  "alternate_arrangement",
  "related_performance",
] as const satisfies readonly S["RelationshipKind"][];
export const ARTIST_KINDS = ["person", "group", "other"] as const satisfies readonly S["ArtistKind"][];
export const ALBUM_KINDS = ["album", "single", "ep", "compilation", "soundtrack", "other"] as const satisfies readonly S["AlbumKind"][];

type Load<T> = { status: "loading" } | { status: "error"; error: unknown } | { status: "ready"; data: T };

/** Loads `load()` whenever `key` changes; `reload` refetches (e.g. after a conflict). */
export function useLoad<T>(key: string, load: () => Promise<T>) {
  const loadRef = useRef(load);
  loadRef.current = load;
  const [state, setState] = useState<Load<T>>({ status: "loading" });
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    let live = true;
    setState({ status: "loading" });
    loadRef.current().then(
      (data) => live && setState({ status: "ready", data }),
      (error) => live && setState({ status: "error", error }),
    );
    return () => {
      live = false;
    };
  }, [key, nonce]);
  return { state, reload: () => setNonce((n) => n + 1) };
}

/** Renders loading/error states around a loaded value. */
export function Loaded<T>({ load, children }: { load: ReturnType<typeof useLoad<T>>; children: (data: T) => React.ReactNode }) {
  const { state, reload } = load;
  if (state.status === "loading") return <LoadingState />;
  if (state.status === "error") return <ErrorState error={state.error} onRetry={reload} />;
  return <>{children(state.data)}</>;
}

type PageOf<T> = { items: T[]; next_cursor?: string | null };

/** Cursor-paged list with "load more". Restarts from the first page when `key` changes. */
export function usePaged<T>(key: string, fetchPage: (cursor?: string) => Promise<PageOf<T>>) {
  const fetchRef = useRef(fetchPage);
  fetchRef.current = fetchPage;
  const [items, setItems] = useState<T[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "more" | "error">("loading");
  const [error, setError] = useState<unknown>();
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let live = true;
    setStatus("loading");
    setItems([]);
    fetchRef.current().then(
      (p) => {
        if (!live) return;
        setItems(p.items);
        setCursor(p.next_cursor ?? null);
        setStatus("ready");
      },
      (e) => {
        if (!live) return;
        setError(e);
        setStatus("error");
      },
    );
    return () => {
      live = false;
    };
  }, [key, nonce]);

  const loadMore = async () => {
    if (!cursor || status !== "ready") return;
    setStatus("more");
    try {
      const p = await fetchRef.current(cursor);
      setItems((xs) => [...xs, ...p.items]);
      setCursor(p.next_cursor ?? null);
      setStatus("ready");
    } catch (e) {
      setError(e);
      setStatus("error");
    }
  };
  return { items, status, error, hasMore: cursor !== null, loadMore, retry: () => setNonce((n) => n + 1) };
}

/** Names and titles as written: own direction, never machine-translated. */
export function Verbatim({ children }: { children: string }) {
  return (
    <span dir="auto" translate="no">
      {children}
    </span>
  );
}

export function Duration({ ms }: { ms?: number | null }) {
  if (ms == null) return null;
  const s = Math.round(ms / 1000);
  const clock = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  return <bdi dir="ltr">{clock}</bdi>;
}

/** Shared list chrome: prefix filter, states, items, load more. */
export function ListShell<T extends { id: string }>({
  kind,
  list,
  prefix,
  onFilter,
  newHref,
  children,
  renderItem,
}: {
  kind: Kind;
  list: ReturnType<typeof usePaged<T>>;
  prefix: string;
  /** Receives the filter form's data (always includes `prefix`). */
  onFilter: (data: FormData) => void;
  newHref?: string;
  children?: React.ReactNode;
  renderItem: (item: T) => React.ReactNode;
}) {
  const t = useTranslations("catalogue");
  const tc = useTranslations("common");
  const id = useId();
  return (
    <section>
      <p className="actions">
        <Link className="button primary" href={newHref ?? `/${kind}/new`}>
          {t(`${kind}.new`)}
        </Link>
      </p>
      <form
        role="search"
        className="filters"
        onSubmit={(e) => {
          e.preventDefault();
          onFilter(new FormData(e.currentTarget));
        }}
      >
        <div className="field">
          <label htmlFor={id}>{t("filterPrefix")}</label>
          <input id={id} name="prefix" type="search" defaultValue={prefix} />
        </div>
        {children}
        <button className="button" type="submit">
          {t("applyFilter")}
        </button>
      </form>
      {list.status === "loading" ? (
        <LoadingState />
      ) : list.status === "error" && list.items.length === 0 ? (
        <ErrorState error={list.error} onRetry={list.retry} />
      ) : list.items.length === 0 ? (
        <EmptyState title={t(`${kind}.empty`)} />
      ) : (
        <>
          <ul className="entity-list">
            {list.items.map((item) => (
              <li key={item.id}>{renderItem(item)}</li>
            ))}
          </ul>
          {list.status === "error" && <ProblemAlert error={list.error} />}
          {list.hasMore && (
            <button className="button" type="button" onClick={list.loadMore} disabled={list.status === "more"} aria-busy={list.status === "more"}>
              {list.status === "more" ? tc("loading") : t("loadMore")}
            </button>
          )}
        </>
      )}
    </section>
  );
}

export type PickOption = { id: string; label: string; note?: string };

/** Finds an existing record by name/title prefix. Never creates one implicitly. */
export function Picker({ label, search, onPick }: { label: string; search: (prefix: string) => Promise<PickOption[]>; onPick: (o: PickOption) => void }) {
  const t = useTranslations("catalogue");
  const id = useId();
  const [query, setQuery] = useState("");
  const [state, setState] = useState<{ status: "idle" | "loading" } | { status: "done"; options: PickOption[] } | { status: "error"; error: unknown }>({ status: "idle" });
  const run = async () => {
    if (!query.trim()) return;
    setState({ status: "loading" });
    try {
      setState({ status: "done", options: await search(query) });
    } catch (error) {
      setState({ status: "error", error });
    }
  };
  return (
    <div className="picker">
      <label htmlFor={id}>{label}</label>
      <div className="picker-row">
        <input
          id={id}
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void run();
            }
          }}
        />
        <button className="button" type="button" onClick={run} disabled={state.status === "loading"}>
          {t("find")}
        </button>
      </div>
      {state.status === "loading" && <LoadingState />}
      {state.status === "error" && <ProblemAlert error={state.error} />}
      {state.status === "done" &&
        (state.options.length === 0 ? (
          <p className="muted small" role="status">
            {t("noMatches")}
          </p>
        ) : (
          <ul className="picker-results" aria-label={t("matches")}>
            {state.options.map((o) => (
              <li key={o.id}>
                <button
                  type="button"
                  className="button"
                  onClick={() => {
                    onPick(o);
                    setState({ status: "idle" });
                    setQuery("");
                  }}
                >
                  <Verbatim>{o.label}</Verbatim>
                  {o.note && <span className="muted small"> {o.note}</span>}
                </button>
              </li>
            ))}
          </ul>
        ))}
    </div>
  );
}

/** An editable, ordered list of picked records, each with an optional role/kind select. */
export function RefListEditor<R extends string>({
  name,
  legend,
  items,
  onChange,
  choices,
  choiceLabels,
  pickerLabel,
  search,
  entity,
  fieldErrors,
  ordered,
}: {
  name: string;
  legend: string;
  items: { id: string; label: string; choice?: R }[];
  onChange: (items: { id: string; label: string; choice?: R }[]) => void;
  choices?: readonly R[];
  choiceLabels?: string;
  pickerLabel: string;
  /** Free-text prefix search, for records that are not shared entities (recordings). */
  search?: (prefix: string) => Promise<PickOption[]>;
  /** Shared entity types use the one EntityPicker: the stored value is the id. */
  entity?: EntityType;
  fieldErrors: FieldErrors;
  ordered?: boolean;
}) {
  const t = useTranslations();
  const errId = useId();
  const err = useFieldError(fieldErrors, name);
  const update = (i: number, patch: Partial<(typeof items)[number]>) => onChange(items.map((x, j) => (j === i ? { ...x, ...patch } : x)));
  const move = (i: number, d: -1 | 1) => {
    const next = [...items];
    [next[i], next[i + d]] = [next[i + d], next[i]];
    onChange(next);
  };
  return (
    <fieldset className="ref-list" aria-invalid={err ? true : undefined} aria-describedby={err ? errId : undefined}>
      <legend>{legend}</legend>
      {err && (
        <p id={errId} className="field-error">
          {err}
        </p>
      )}
      {items.length > 0 && (
        <ol className="ref-items">
          {items.map((item, i) => (
            <li key={`${item.id}-${i}`}>
              <Verbatim>{item.label}</Verbatim>
              {choices && choiceLabels && (
                <select
                  aria-label={t("catalogue.choiceFor", { name: item.label })}
                  value={item.choice}
                  onChange={(e) => update(i, { choice: e.target.value as R })}
                >
                  {choices.map((c) => (
                    <option key={c} value={c}>
                      {t(`${choiceLabels}.${c}` as string)}
                    </option>
                  ))}
                </select>
              )}
              {ordered && (
                <>
                  <button type="button" className="button" onClick={() => move(i, -1)} disabled={i === 0} aria-label={t("catalogue.moveUp", { name: item.label })}>
                    {t("catalogue.up")}
                  </button>
                  <button type="button" className="button" onClick={() => move(i, 1)} disabled={i === items.length - 1} aria-label={t("catalogue.moveDown", { name: item.label })}>
                    {t("catalogue.down")}
                  </button>
                </>
              )}
              <button type="button" className="button" onClick={() => onChange(items.filter((_, j) => j !== i))} aria-label={t("catalogue.removeItem", { name: item.label })}>
                {t("catalogue.remove")}
              </button>
            </li>
          ))}
        </ol>
      )}
      {entity ? (
        <EntityPicker type={entity} label={pickerLabel} onPick={(o) => onChange([...items, { id: o.id, label: o.label, choice: choices?.[0] }])} />
      ) : (
        search && <Picker label={pickerLabel} search={search} onPick={(o) => onChange([...items, { id: o.id, label: o.label, choice: choices?.[0] }])} />
      )}
    </fieldset>
  );
}

/** Errors for a save: conflicts offer reload, duplicates link to the existing record. */
export function SaveProblem({ error, kind, onReload }: { error: unknown; kind: Kind; onReload?: () => void }) {
  const t = useTranslations("catalogue");
  const existing = error instanceof ApiProblem && error.code === "duplicate_identifier" ? error.existingId : undefined;
  return (
    <>
      <ProblemAlert error={error} onReload={onReload} />
      {existing && (
        <p>
          <Link href={`/${kind}/${existing}`}>{t("viewExisting")}</Link>
        </p>
      )}
    </>
  );
}

/** Blank → null (clears on PATCH, absent on create). */
export const text = (v: string | undefined) => (v && v.trim() ? v.trim() : null);
/** Blank → null; non-numeric input is sent as-is so the server reports it. */
export const int = (v: string | undefined) => (v && v.trim() ? Number(v) : null);

export type FieldProblem = Problem["errors"];

export const str = (fd: FormData, k: string) => String(fd.get(k) ?? "");

/** Optional enum filter for a list. */
export function KindFilter({ value, options, labels }: { value: string; options: readonly string[]; labels: string }) {
  const t = useTranslations();
  const id = useId();
  return (
    <div className="field">
      <label htmlFor={id}>{t("fields.kind")}</label>
      <select id={id} name="kind" defaultValue={value}>
        <option value="">{t("catalogue.anyKind")}</option>
        {options.map((o) => (
          <option key={o} value={o}>
            {t(`${labels}.${o}`)}
          </option>
        ))}
      </select>
    </div>
  );
}

/** Submit + cancel for catalogue forms. */
export function FormActions({ submitting, cancelHref, disabled }: { submitting: boolean; cancelHref: string; disabled?: boolean }) {
  const t = useTranslations();
  return (
    <p className="actions">
      <button className="button primary" type="submit" disabled={submitting || disabled} aria-busy={submitting}>
        {submitting ? t("common.submitting") : t("catalogue.save")}
      </button>
      <Link className="button" href={cancelHref}>
        {t("catalogue.cancel")}
      </Link>
    </p>
  );
}

export const ROLE_CATEGORIES = ["original_songwriter", "translator", "adapter", "performance", "production", "other"] as const;

/** Groups by role category so original songwriters, translators and adapters never run together. */
export function groupByCategory<T extends { role_category: string }>(items: T[]): [string, T[]][] {
  return ROLE_CATEGORIES.map((c): [string, T[]] => [c, items.filter((i) => i.role_category === c)]).filter(([, xs]) => xs.length > 0);
}

export function CreditList({ credits }: { credits: S["Credit"][] }) {
  const t = useTranslations("catalogue");
  if (credits.length === 0) return <p className="muted">{t("noCredits")}</p>;
  return (
    <>
      {groupByCategory(credits).map(([category, group]) => (
        <section key={category} data-category={category} aria-label={t(`roleCategories.${category}`)}>
          <h3>{t(`roleCategories.${category}`)}</h3>
          <ul className="credits">
            {group.map((c) => (
              <li key={`${c.artist_id}-${c.role}`}>
                <Link href={`/artists/${c.artist_id}`}>
                  <Name display={c.artist_display} />
                </Link>
                <span className="muted">
                  {" "}
                  <Name display={c.role_display} />
                </span>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </>
  );
}
