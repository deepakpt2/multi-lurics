"use client";

import { useCallback, useEffect, useId, useState } from "react";
import { useFormatter, useTranslations } from "next-intl";
import { ApiProblem, api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { usePaged } from "../catalogue/common";
import { EmptyState, LoadingState, ProblemAlert } from "../states";
import { Link } from "@/i18n/navigation";

type S = components["schemas"];
const REACTIONS = ["like", "moving", "insightful"] as const;
const REASONS = ["spam", "abuse", "copyright", "inaccurate", "other"] as const;

/** A validation code from the server (spam heuristics, length) in the member's language. */
function useCodeMessage() {
  const t = useTranslations("community.codes");
  return (error: unknown): string | null => {
    if (!(error instanceof ApiProblem)) return null;
    const code = error.problem.errors?.[0]?.code;
    return code && t.has(code) ? t(code) : null;
  };
}

function Failure({ error, onReload }: { error: unknown; onReload?: () => void }) {
  const code = useCodeMessage()(error);
  const t = useTranslations("errors");
  if (error instanceof ApiProblem && error.code === "rate_limited") {
    const wait = error.problem.retry_after_secs;
    return (
      <div className="alert alert-warning" role="alert" data-state="rate-limited">
        <p>{wait ? t("rate_limited_wait", { seconds: wait }) : t("slow_down")}</p>
      </div>
    );
  }
  if (code) {
    return (
      <div className="alert alert-error" role="alert" data-state="error">
        <p>{code}</p>
      </div>
    );
  }
  return <ProblemAlert error={error} onReload={onReload} />;
}

export function ReportButton({ targetType, targetId }: { targetType: "contribution" | "comment"; targetId: string }) {
  const t = useTranslations("community");
  const uid = useId();
  const [open, setOpen] = useState(false);
  const [reason, setReason] = useState<(typeof REASONS)[number]>("spam");
  const [detail, setDetail] = useState("");
  const [state, setState] = useState<"idle" | "sending" | "sent">("idle");
  const [error, setError] = useState<unknown>();
  if (state === "sent") return <span role="status" className="muted small">{t("reportSent")}</span>;
  if (!open)
    return (
      <button type="button" className="button link" onClick={() => setOpen(true)}>
        {t("report")}
      </button>
    );
  return (
    <form
      className="report-form"
      onSubmit={async (e) => {
        e.preventDefault();
        setState("sending");
        setError(undefined);
        try {
          await call(api.POST("/api/v1/reports", { body: { target_type: targetType, target_id: targetId, reason, detail: detail.trim() || null } }));
          setState("sent");
        } catch (err) {
          setError(err);
          setState("idle");
        }
      }}
    >
      <div className="field">
        <label htmlFor={`${uid}-r`}>{t("reason")}</label>
        <select id={`${uid}-r`} value={reason} onChange={(e) => setReason(e.target.value as (typeof REASONS)[number])}>
          {REASONS.map((r) => (
            <option key={r} value={r}>{t(`reasons.${r}`)}</option>
          ))}
        </select>
      </div>
      <div className="field">
        <label htmlFor={`${uid}-d`}>{t("reportDetail")}</label>
        <textarea id={`${uid}-d`} value={detail} rows={2} dir="auto" onChange={(e) => setDetail(e.target.value)} />
      </div>
      {error !== undefined && <Failure error={error} />}
      <button type="submit" className="button primary" disabled={state === "sending"}>{t("sendReport")}</button>{" "}
      <button type="button" className="button" onClick={() => setOpen(false)}>{t("cancel")}</button>
    </form>
  );
}

function Comments({ id }: { id: string }) {
  const t = useTranslations("community");
  const fmt = useFormatter();
  const uid = useId();
  const list = usePaged<S["Comment"]>(`comments:${id}`, (cursor) => call(api.GET("/api/v1/contributions/{id}/comments", { params: { path: { id }, query: { cursor } } })));
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>();

  return (
    <section aria-labelledby={`${uid}-h`}>
      <h2 id={`${uid}-h`}>{t("comments")}</h2>
      {list.status === "loading" && <LoadingState />}
      {list.status === "error" && <ProblemAlert error={list.error} onReload={list.retry} />}
      {list.status !== "loading" && list.status !== "error" && list.items.length === 0 && <EmptyState title={t("noComments")} />}
      <ul className="comments">
        {list.items.map((c) => (
          <li key={c.id} className="comment">
            <p className="meta">
              <Link href={`/users/${c.author_id}`} dir="auto">{c.author_name}</Link>{" "}
              <time dateTime={c.created_at}>{fmt.dateTime(new Date(c.created_at), { dateStyle: "medium" })}</time>
            </p>
            <p dir="auto" className="comment-body">{c.body}</p>
            <ReportButton targetType="comment" targetId={c.id} />
          </li>
        ))}
      </ul>
      {list.hasMore && (
        <button type="button" className="button" onClick={list.loadMore} disabled={list.status === "more"}>{t("more")}</button>
      )}
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (busy) return;
          setBusy(true);
          setError(undefined);
          try {
            await call(api.POST("/api/v1/contributions/{id}/comments", { params: { path: { id } }, body: { body } }));
            setBody("");
            list.retry();
          } catch (err) {
            setError(err);
          } finally {
            setBusy(false);
          }
        }}
      >
        <div className="field">
          <label htmlFor={`${uid}-c`}>{t("addComment")}</label>
          <textarea id={`${uid}-c`} value={body} rows={3} dir="auto" onChange={(e) => setBody(e.target.value)} required />
        </div>
        {error !== undefined && <Failure error={error} />}
        <button type="submit" className="button primary" disabled={busy || !body.trim()}>{t("post")}</button>
      </form>
    </section>
  );
}

/** Ratings, reactions, bookmark, report and comments for one published contribution. */
export function CommunitySection({ contributionId }: { contributionId: string }) {
  const t = useTranslations("community");
  const [data, setData] = useState<S["Engagement"]>();
  const [error, setError] = useState<unknown>();
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setData(await call(api.GET("/api/v1/contributions/{id}/engagement", { params: { path: { id: contributionId } } })));
      setError(undefined);
    } catch (e) {
      setError(e);
    }
  }, [contributionId]);
  useEffect(() => {
    void load();
  }, [load]);

  const act = async (run: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    try {
      await run();
      await load();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  if (!data && !error) return <LoadingState />;
  const mine = data?.mine;
  const params = { params: { path: { id: contributionId } } };
  return (
    <section className="community">
      {error !== undefined && <Failure error={error} onReload={load} />}
      {data && (
        <>
          <div role="group" aria-label={t("rating")} className="rating">
            {[1, 2, 3, 4, 5].map((v) => (
              <button
                key={v}
                type="button"
                className="button"
                aria-pressed={mine?.rating === v}
                aria-label={t("rateN", { n: v })}
                disabled={busy}
                onClick={() => act(() => call(api.PUT("/api/v1/contributions/{id}/rating", { ...params, body: { value: v } })))}
              >
                {v}
              </button>
            ))}
            <span className="muted small" data-testid="rating-summary">
              {data.rating_average != null ? t("ratingSummary", { avg: data.rating_average.toFixed(1), count: data.rating_count }) : t("noRatings")}
            </span>
          </div>
          <div role="group" aria-label={t("reactions")} className="reactions">
            {REACTIONS.map((k) => {
              const on = mine?.reactions.includes(k) ?? false;
              const n = data.reactions.find((r) => r.kind === k)?.count ?? 0;
              return (
                <button
                  key={k}
                  type="button"
                  className="button"
                  aria-pressed={on}
                  disabled={busy}
                  onClick={() =>
                    act(() =>
                      call(
                        on
                          ? api.DELETE("/api/v1/contributions/{id}/reactions/{kind}", { params: { path: { id: contributionId, kind: k } } })
                          : api.PUT("/api/v1/contributions/{id}/reactions/{kind}", { params: { path: { id: contributionId, kind: k } } }),
                      ),
                    )
                  }
                >
                  {t(`reaction.${k}`)} <span className="muted small">{n}</span>
                </button>
              );
            })}
            <button
              type="button"
              className="button"
              aria-pressed={mine?.bookmarked ?? false}
              disabled={busy}
              onClick={() => act(() => call(mine?.bookmarked ? api.DELETE("/api/v1/contributions/{id}/bookmark", params) : api.PUT("/api/v1/contributions/{id}/bookmark", params)))}
            >
              {mine?.bookmarked ? t("bookmarked") : t("bookmark")}
            </button>
            <ReportButton targetType="contribution" targetId={contributionId} />
          </div>
          <Comments id={contributionId} />
        </>
      )}
    </section>
  );
}

// ---------- profile ----------

export function ProfileView({ userId }: { userId: string }) {
  const t = useTranslations("community");
  const fmt = useFormatter();
  const [p, setP] = useState<S["Profile"]>();
  const [error, setError] = useState<unknown>();
  useEffect(() => {
    call(api.GET("/api/v1/users/{id}/profile", { params: { path: { id: userId } } })).then(setP, setError);
  }, [userId]);
  if (error !== undefined) return <ProblemAlert error={error} />;
  if (!p) return <LoadingState />;
  return (
    <article className="profile">
      <h1 dir="auto">{p.display_name}</h1>
      <p className="meta">
        <span className="badge" data-testid="level">{t(`levels.${p.level as "newcomer"}`)}</span>{" "}
        {/* Roles are separate from the level and only ever granted by an administrator. */}
        {p.roles.map((r) => (
          <span key={r} className="badge badge-role" data-testid="role">{t(`roles.${r as "moderator"}`)}</span>
        ))}
      </p>
      {p.bio && <p dir="auto">{p.bio}</p>}
      <dl className="facts">
        <dt>{t("reputation")}</dt>
        <dd>{p.reputation}</dd>
        <dt>{t("published")}</dt>
        <dd>{p.published_contributions}</dd>
        <dt>{t("commentsCount")}</dt>
        <dd>{p.comments}</dd>
        <dt>{t("joined")}</dt>
        <dd>{fmt.dateTime(new Date(p.joined_at), { dateStyle: "medium" })}</dd>
      </dl>
    </article>
  );
}

// ---------- my comments (and appeals) ----------

export function MyComments() {
  const t = useTranslations("community");
  const list = usePaged<S["Comment"]>("my-comments", (cursor) => call(api.GET("/api/v1/me/comments", { params: { query: { cursor } } })));
  const [appealing, setAppealing] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [sent, setSent] = useState<Set<string>>(new Set());
  const [error, setError] = useState<unknown>();
  if (list.status === "loading") return <LoadingState />;
  if (list.status === "error") return <ProblemAlert error={list.error} onReload={list.retry} />;
  if (list.items.length === 0) return <EmptyState title={t("noComments")} />;
  return (
    <ul className="comments">
      {list.items.map((c) => (
        <li key={c.id} className="comment" data-status={c.status}>
          <p dir="auto">{c.body}</p>
          {c.status === "hidden" && (
            <>
              <p className="alert alert-warning">{t("hiddenNotice")}</p>
              {sent.has(c.id) ? (
                <p role="status">{t("appealSent")}</p>
              ) : appealing === c.id ? (
                <form
                  onSubmit={async (e) => {
                    e.preventDefault();
                    setError(undefined);
                    try {
                      await call(api.POST("/api/v1/appeals", { body: { comment_id: c.id, message } }));
                      setSent(new Set(sent).add(c.id));
                      setAppealing(null);
                    } catch (err) {
                      setError(err);
                    }
                  }}
                >
                  <div className="field">
                    <label htmlFor={`appeal-${c.id}`}>{t("appealMessage")}</label>
                    <textarea id={`appeal-${c.id}`} value={message} dir="auto" rows={3} onChange={(e) => setMessage(e.target.value)} required />
                  </div>
                  {error !== undefined && <Failure error={error} />}
                  <button type="submit" className="button primary">{t("appeal")}</button>
                </form>
              ) : (
                <button type="button" className="button" onClick={() => setAppealing(c.id)}>{t("appeal")}</button>
              )}
            </>
          )}
        </li>
      ))}
    </ul>
  );
}
