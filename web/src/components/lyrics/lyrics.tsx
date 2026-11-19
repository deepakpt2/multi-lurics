"use client";

import { useEffect, useId, useRef, useState } from "react";
import { useLocale, useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Link, useRouter } from "@/i18n/navigation";
import { Loaded, usePaged, useLoad } from "../catalogue/common";
import { useFieldError, useSubmit } from "../form";
import { Name, languageName } from "../localization/localized";
import { SelectBox, languageLabel, listLanguages, listScripts, scriptLabel } from "../localization/names";
import { EmptyState, ErrorState, LoadingState, ProblemAlert } from "../states";

type S = components["schemas"];
type Summary = S["ContributionSummary"];
type Detail = S["ContributionDetail"];
const TYPES = ["original", "translation", "transliteration"] as const satisfies readonly S["ContributionType"][];

/** Retry-safe writes: one key per editing session, so a double click or a resend creates one revision. */
const newKey = () => globalThis.crypto?.randomUUID?.() ?? `k-${Date.now()}-${Math.random().toString(36).slice(2)}`;
const idem = (key: string) => ({ "Idempotency-Key": key });

/** The lyrics themselves: language and direction come from the contribution, never from the UI locale. */
function Body({ text, language, dir }: { text: string; language: string; dir: S["Dir"] }) {
  return (
    <pre className="lyrics" lang={language} dir={dir} translate="no">
      {text}
    </pre>
  );
}

function Badges({ c }: { c: Summary }) {
  const t = useTranslations("lyrics");
  const locale = useLocale();
  const where = [languageName(c.language, locale), c.script].join(t("separator"));
  return (
    <p className="meta">
      <span className="badge">{t(`types.${c.type}`)}</span>{" "}
      <span className="badge" lang={c.language}>
        {where}
      </span>{" "}
      <span className="badge" data-primary={c.is_primary}>
        {c.is_primary ? t("primary") : t("alternative")}
      </span>{" "}
      <span className="badge" data-status={c.status}>
        {t(`status.${c.status}`)}
      </span>
    </p>
  );
}

/** Is the signed-in user a moderator? Only used to decide which buttons to offer; the server decides. */
function useIsModerator() {
  const [mod, setMod] = useState(false);
  useEffect(() => {
    let live = true;
    call(api.GET("/api/v1/me")).then(
      (u) => live && setMod(u.roles.some((r) => r === "moderator" || r === "admin")),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);
  return mod;
}

const loadDetail = (id: string) => call(api.GET("/api/v1/contributions/{id}", { params: { path: { id } } }));

// ---------- recording page: published lyrics ----------

export function LyricsSection({ recordingId }: { recordingId: string }) {
  const t = useTranslations("lyrics");
  const load = useLoad(recordingId, () =>
    call(api.GET("/api/v1/recordings/{id}/contributions", { params: { path: { id: recordingId }, query: { limit: 100 } } })),
  );
  return (
    <section aria-labelledby="lyrics-heading">
      <h2 id="lyrics-heading">{t("title")}</h2>
      <Loaded load={load}>
        {(page) =>
          page.items.length === 0 ? (
            <p className="muted">{t("none")}</p>
          ) : (
            <ul className="entity-list">
              {page.items.map((c) => (
                <li key={c.id}>
                  <Link href={`/contributions/${c.id}`}>{t(`types.${c.type}`)}</Link> <Badges c={c} />
                </li>
              ))}
            </ul>
          )
        }
      </Loaded>
      <p className="actions">
        <Link className="button" href={`/recordings/${recordingId}/lyrics/new`}>
          {t("add")}
        </Link>
      </p>
    </section>
  );
}

// ---------- create ----------

function BodyField({ rtl, lang, fieldErrors, defaultValue }: { rtl: boolean; lang?: string; fieldErrors: { field: string; code: string }[]; defaultValue?: string }) {
  const t = useTranslations();
  const id = useId();
  const err = useFieldError(fieldErrors, "body");
  return (
    <div className="field">
      <label htmlFor={id}>{t("fields.body")}</label>
      <textarea
        id={id}
        name="body"
        className="lyrics-input"
        rows={16}
        dir={rtl ? "rtl" : "ltr"}
        lang={lang}
        defaultValue={defaultValue}
        translate="no"
        required
        aria-invalid={err ? true : undefined}
        aria-describedby={err ? `${id}-err` : `${id}-hint`}
      />
      <p id={`${id}-hint`} className="muted small">
        {t("lyrics.bodyHint")}
      </p>
      {err && (
        <p id={`${id}-err`} className="field-error">
          {err}
        </p>
      )}
    </div>
  );
}

const textOf = (fd: FormData, k: string) => String(fd.get(k) ?? "");

export function ContributionNew({ recordingId, query }: { recordingId: string; query: Record<string, string> }) {
  const t = useTranslations("lyrics");
  const tf = useTranslations("fields");
  const router = useRouter();
  const key = useRef(newKey());
  const refs = useLoad("refs", async () => ({ languages: (await listLanguages()).items, scripts: (await listScripts()).items }));
  const [type, setType] = useState<string>(query.type ?? "original");
  const [language, setLanguage] = useState(query.language ?? "");
  const [script, setScript] = useState(query.script ?? "");
  const s = useSubmit(async (fd: FormData) => {
    const note = textOf(fd, "note").trim();
    const created = await call(
      api.POST("/api/v1/recordings/{id}/contributions", {
        params: { path: { id: recordingId }, header: idem(key.current) },
        body: {
          type,
          language,
          script,
          body: textOf(fd, "body"),
          note: note || null,
          source_contribution_id: query.source || null,
        },
      }),
    );
    router.push(`/contributions/${created.id}`);
  });
  return (
    <Loaded load={refs}>
      {({ languages, scripts }) => {
        const lang = languages.find((l) => l.iso639_3 === language || l.iso639_1 === language);
        const scr = scripts.find((x) => x.iso15924 === script);
        const rtl = scr?.is_rtl ?? lang?.is_rtl ?? false;
        return (
          <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(new FormData(e.currentTarget)))} aria-label={t("newTitle")}>
            {s.error !== undefined && <ProblemAlert error={s.error} />}
            <SelectBox name="type" label={tf("type")} value={type} onChange={setType} fieldErrors={s.fieldErrors}>
              {TYPES.map((x) => (
                <option key={x} value={x}>
                  {t(`types.${x}`)}
                </option>
              ))}
            </SelectBox>
            <SelectBox
              name="language"
              label={tf("language")}
              value={language}
              onChange={(v) => {
                setLanguage(v);
                // Language and script are separate choices; the language only suggests a usual script.
                const next = languages.find((l) => l.iso639_3 === v);
                if (next?.default_script && !script) setScript(next.default_script);
              }}
              fieldErrors={s.fieldErrors}
            >
              <option value="">{t("choose")}</option>
              {languages.map((l) => (
                <option key={l.id} value={l.iso639_3}>
                  {languageLabel(l)}
                </option>
              ))}
            </SelectBox>
            <SelectBox name="script" label={tf("script")} value={script} onChange={setScript} fieldErrors={s.fieldErrors}>
              <option value="">{t("choose")}</option>
              {scripts.map((x) => (
                <option key={x.id} value={x.iso15924}>
                  {scriptLabel(x)}
                </option>
              ))}
            </SelectBox>
            <BodyField rtl={rtl} lang={lang ? (lang.iso639_1 ?? lang.iso639_3) : undefined} fieldErrors={s.fieldErrors} />
            <div className="field">
              <label>
                {tf("note")}
                <input name="note" type="text" />
              </label>
              <p className="muted small">{t("noteHint")}</p>
            </div>
            <button className="button primary" type="submit" disabled={s.submitting || !language || !script} aria-busy={s.submitting}>
              {s.submitting ? t("submitting") : t("save")}
            </button>
          </form>
        );
      }}
    </Loaded>
  );
}

// ---------- detail and moderation actions ----------

function Decisions({ c, onDone }: { c: Detail; onDone: () => void }) {
  const t = useTranslations("lyrics");
  const [note, setNote] = useState("");
  const s = useSubmit(async (verb: "approve" | "publish" | "reject") => {
    const path = { id: c.id };
    const body = { expected_parent_revision_id: c.current_revision_id, note: note.trim() || null };
    const key = newKey();
    const header = idem(key);
    if (verb === "approve") await call(api.POST("/api/v1/contributions/{id}/approve", { params: { path, header }, body }));
    else if (verb === "publish") await call(api.POST("/api/v1/contributions/{id}/publish", { params: { path, header }, body }));
    else await call(api.POST("/api/v1/contributions/{id}/reject", { params: { path, header }, body }));
    onDone();
  });
  return (
    <section aria-label={t("reviewTitle")} className="queue-item">
      <h2>{t("reviewTitle")}</h2>
      {s.error !== undefined && <ProblemAlert error={s.error} onReload={onDone} />}
      <div className="field">
        <label>
          {t("reviewNote")}
          <input type="text" value={note} onChange={(e) => setNote(e.target.value)} />
        </label>
      </div>
      <p className="actions">
        {c.status === "pending" && (
          <button className="button" type="button" disabled={s.submitting} onClick={() => s.submit("approve")}>
            {t("approve")}
          </button>
        )}
        <button className="button primary" type="button" disabled={s.submitting} onClick={() => s.submit("publish")}>
          {t("publish")}
        </button>
        <button className="button" type="button" disabled={s.submitting} onClick={() => s.submit("reject")}>
          {t("reject")}
        </button>
      </p>
    </section>
  );
}

export function ContributionDetail({ id }: { id: string }) {
  const t = useTranslations("lyrics");
  const mod = useIsModerator();
  const load = useLoad(id, async () => {
    const c = await loadDetail(id);
    const recording = await call(api.GET("/api/v1/recordings/{id}", { params: { path: { id: c.recording_id } } }));
    return { c, recording };
  });
  return (
    <Loaded load={load}>
      {({ c, recording }) => {
        const mine = c.current_revision_id != null; // only the author and moderators receive it
        const awaiting = c.status === "pending" || c.status === "approved";
        return (
          <article>
            <h1>
              <Link href={`/recordings/${c.recording_id}`}>
                <Name display={recording.display} />
              </Link>
            </h1>
            <Badges c={c} />
            {c.source_contribution_id && (
              <p>
                <Link href={`/contributions/${c.source_contribution_id}`}>{t("openSource")}</Link>
              </p>
            )}
            {c.published ? (
              <Body text={c.published.body} language={c.language} dir={c.dir} />
            ) : (
              <p className="muted">{t("noPublished")}</p>
            )}
            {c.has_unpublished_changes && c.head && (
              <section className="alert alert-warning" data-state="pending-changes">
                <h2>{t("latestRevision")}</h2>
                <p>{t("unpublishedChanges")}</p>
                {c.review_note && <p>{t("reviewNoteShown", { note: c.review_note })}</p>}
                <Body text={c.head.body} language={c.language} dir={c.dir} />
              </section>
            )}
            <p className="actions">
              {mine && (
                <Link className="button" href={`/contributions/${id}/edit`}>
                  {t("edit")}
                </Link>
              )}
              <Link className="button" href={`/contributions/${id}/history`}>
                {t("history")}
              </Link>
              <Link
                className="button"
                href={`/recordings/${c.recording_id}/lyrics/new?type=${c.type}&language=${c.language}&script=${c.script}`}
              >
                {t("addAlternative")}
              </Link>
            </p>
            {mod && awaiting && <Decisions c={c} onDone={load.reload} />}
          </article>
        );
      }}
    </Loaded>
  );
}

// ---------- edit with conflict handling ----------

function Editor({ c, onReload }: { c: Detail; onReload: () => void }) {
  const t = useTranslations("lyrics");
  const tf = useTranslations("fields");
  const router = useRouter();
  const key = useRef(newKey());
  const draft = useRef("");
  const [conflictDraft, setConflictDraft] = useState<string | null>(null);
  const s = useSubmit(async (fd: FormData) => {
    draft.current = textOf(fd, "body");
    const note = textOf(fd, "note").trim();
    try {
      await call(
        api.POST("/api/v1/contributions/{id}/revisions", {
          params: { path: { id: c.id }, header: idem(key.current) },
          body: { expected_parent_revision_id: c.current_revision_id, body: draft.current, note: note || null },
        }),
      );
    } catch (e) {
      setConflictDraft(draft.current);
      throw e;
    }
    router.push(`/contributions/${c.id}`);
  });
  const initial = c.head?.body ?? c.published?.body ?? "";
  return (
    <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(new FormData(e.currentTarget)))} aria-label={t("editTitle")}>
      {s.error !== undefined && <ProblemAlert error={s.error} onReload={onReload} />}
      {conflictDraft !== null && s.error !== undefined && (
        <details open>
          <summary>{t("yourUnsavedText")}</summary>
          <Body text={conflictDraft} language={c.language} dir={c.dir} />
        </details>
      )}
      <BodyField rtl={c.dir === "rtl"} lang={c.language} fieldErrors={s.fieldErrors} defaultValue={initial} />
      <div className="field">
        <label>
          {tf("note")}
          <input name="note" type="text" />
        </label>
      </div>
      <button className="button primary" type="submit" disabled={s.submitting} aria-busy={s.submitting}>
        {s.submitting ? t("submitting") : t("save")}
      </button>
    </form>
  );
}

export function ContributionEdit({ id }: { id: string }) {
  const t = useTranslations("lyrics");
  const load = useLoad(id, () => loadDetail(id));
  return (
    <Loaded load={load}>
      {(c) =>
        c.current_revision_id == null ? (
          <EmptyState title={t("cannotEdit")}>
            <Link href={`/recordings/${c.recording_id}/lyrics/new?type=${c.type}&language=${c.language}&script=${c.script}`}>{t("addAlternative")}</Link>
          </EmptyState>
        ) : (
          // Keyed by the head: loading the latest version replaces the editor's text.
          <Editor key={c.current_revision_id} c={c} onReload={load.reload} />
        )
      }
    </Loaded>
  );
}

// ---------- history and diff ----------

function Diff({ from, to }: { from: string; to: string }) {
  const t = useTranslations("lyrics");
  const load = useLoad(`${from}|${to}`, () =>
    call(api.GET("/api/v1/revisions/{a}/diff/{b}", { params: { path: { a: from, b: to } } })),
  );
  return (
    <Loaded load={load}>
      {(d) => (
        <div className="diff" role="group" aria-label={t("diffTitle")}>
          {d.hunks.map((h, i) =>
            h.lines.map((line, j) => {
              const Tag = h.op === "insert" ? "ins" : h.op === "delete" ? "del" : "span";
              return (
                <p key={`${i}-${j}`} className="diff-line" data-op={h.op} dir="auto">
                  <span aria-hidden="true">{h.op === "insert" ? "+" : h.op === "delete" ? "−" : " "}</span>
                  <Tag>{line || " "}</Tag>
                </p>
              );
            }),
          )}
        </div>
      )}
    </Loaded>
  );
}

export function ContributionHistory({ id }: { id: string }) {
  const t = useTranslations("lyrics");
  const detail = useLoad(id, () => loadDetail(id));
  const list = usePaged<S["RevisionSummary"]>(id, (cursor) =>
    call(api.GET("/api/v1/contributions/{id}/revisions", { params: { path: { id }, query: { cursor } } })),
  );
  const [shown, setShown] = useState<{ from: string; to: string } | null>(null);
  const restore = useSubmit(async (c: Detail, revisionId: string) => {
    await call(
      api.POST("/api/v1/contributions/{id}/revisions", {
        params: { path: { id }, header: idem(newKey()) },
        body: { expected_parent_revision_id: c.current_revision_id, restore_from_revision_id: revisionId },
      }),
    );
    detail.reload();
    list.retry();
  });
  if (list.status === "loading") return <LoadingState />;
  if (list.status === "error" && list.items.length === 0) return <ErrorState error={list.error} onRetry={list.retry} />;
  const c = detail.state.status === "ready" ? detail.state.data : null;
  return (
    <>
      {restore.error !== undefined && <ProblemAlert error={restore.error} onReload={() => (detail.reload(), list.retry())} />}
      {restore.state.status === "success" && (
        <div className="alert alert-success" role="status" data-state="success">
          <p>{t("restored")}</p>
        </div>
      )}
      <ol className="history">
        {list.items.map((r) => (
          <li key={r.id} data-published={r.published}>
            <p>
              <strong>{t("revisionNo", { n: r.revision_no })}</strong>
              {r.id === c?.current_revision_id && <span className="badge"> {t("current")}</span>}
              {r.published && <span className="badge"> {t("wasPublished")}</span>}
              {r.restored_from_revision_id && <span className="badge"> {t("restoredBadge")}</span>}
            </p>
            {r.note && (
              <p className="muted" dir="auto">
                {r.note}
              </p>
            )}
            <p className="actions">
              {r.parent_revision_id && (
                <button type="button" className="button" onClick={() => setShown({ from: r.parent_revision_id!, to: r.id })}>
                  {t("showChanges")}
                </button>
              )}
              {c?.current_revision_id != null && r.id !== c.current_revision_id && (
                <button type="button" className="button" disabled={restore.submitting} onClick={() => restore.submit(c, r.id)}>
                  {t("restore")}
                </button>
              )}
            </p>
            {shown?.to === r.id && <Diff from={shown.from} to={shown.to} />}
          </li>
        ))}
      </ol>
      {list.hasMore && (
        <button className="button" type="button" onClick={list.loadMore}>
          {t("loadMore")}
        </button>
      )}
    </>
  );
}

// ---------- queues ----------

function ContributionRows({ items, action }: { items: Summary[]; action: string }) {
  return (
    <ul className="queue">
      {items.map((c) => (
        <li key={c.id} className="queue-item">
          <Badges c={c} />
          <p>
            <Link href={`/contributions/${c.id}`}>{action}</Link>
          </p>
        </li>
      ))}
    </ul>
  );
}

function PagedContributions({ path, empty, action }: { path: "/api/v1/moderation/contributions" | "/api/v1/me/contributions"; empty: string; action: string }) {
  const t = useTranslations("lyrics");
  const list = usePaged<Summary>(path, (cursor) => call(api.GET(path, { params: { query: { cursor } } })));
  if (list.status === "loading") return <LoadingState />;
  if (list.status === "error" && list.items.length === 0) return <ErrorState error={list.error} onRetry={list.retry} />;
  if (list.items.length === 0) return <EmptyState title={empty} />;
  return (
    <>
      <ContributionRows items={list.items} action={action} />
      {list.hasMore && (
        <button className="button" type="button" onClick={list.loadMore}>
          {t("loadMore")}
        </button>
      )}
    </>
  );
}

/** Heads awaiting a decision; open one to read it and decide. */
export function LyricsQueue() {
  const t = useTranslations("lyrics");
  return <PagedContributions path="/api/v1/moderation/contributions" empty={t("queueEmpty")} action={t("review")} />;
}

export function MyContributions() {
  const t = useTranslations("lyrics");
  return <PagedContributions path="/api/v1/me/contributions" empty={t("mineEmpty")} action={t("open")} />;
}
