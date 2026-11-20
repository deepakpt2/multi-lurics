"use client";

import { useCallback, useEffect, useState } from "react";
import { useFormatter, useTranslations } from "next-intl";
import { Pencil } from "lucide-react";
import { api, call } from "@/api/client";
import { Link } from "@/i18n/navigation";
import { articleCover, fromLocalInput, toLocalInput, type Article } from "@/lib/articles";
import { useSession } from "../session";
import { ErrorState, LoadingState, ProblemAlert } from "../states";
import { ArticleBody } from "./markdown";

/** Moderators choose whether the article is on the home page, and for how long. */
function HomeControls({ article, onChange }: { article: Article; onChange: (a: Article) => void }) {
  const t = useTranslations("articles");
  const [from, setFrom] = useState(toLocalInput(article.home_from));
  const [until, setUntil] = useState(toLocalInput(article.home_until));
  const [error, setError] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  const published = article.status === "published";
  const run = async (fn: () => Promise<Article>) => {
    if (busy) return;
    setBusy(true);
    try {
      const a = await fn();
      setError(undefined);
      setFrom(toLocalInput(a.home_from));
      setUntil(toLocalInput(a.home_until));
      onChange(a);
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  const setHome = (on: boolean) =>
    run(() =>
      call(
        api.PUT("/api/v1/articles/{key}/home", {
          params: { path: { key: article.id } },
          body: { on_home: on, home_from: on ? fromLocalInput(from) : null, home_until: on ? fromLocalInput(until) : null },
        }),
      ),
    );
  const setPublished = (publish: boolean) =>
    run(() =>
      call(api.PATCH("/api/v1/articles/{key}", { params: { path: { key: article.id } }, body: { expected_version: article.version, publish } })),
    );
  const status = article.home_now ? t("homeNow") : article.on_home ? (article.home_from && new Date(article.home_from) > new Date() ? t("homeScheduled") : t("homeEnded")) : t("homeOff");
  return (
    <section className="article-admin" aria-label={t("manage")}>
      <h2>{t("manage")}</h2>
      {error !== undefined && <ProblemAlert error={error} />}
      <p>
        <strong>{published ? t("published") : t("draft")}</strong>{" "}
        <button type="button" className="button" disabled={busy} onClick={() => void setPublished(!published)}>
          {published ? t("unpublish") : t("publish")}
        </button>
      </p>
      <p className="muted" role="status">
        {t("homeStatus", { status })}
      </p>
      {published ? (
        <div className="article-home-form">
          <div className="field">
            <label htmlFor="art-from">{t("homeFrom")}</label>
            <input id="art-from" type="datetime-local" value={from} onChange={(e) => setFrom(e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="art-until">{t("homeUntil")}</label>
            <input id="art-until" type="datetime-local" value={until} onChange={(e) => setUntil(e.target.value)} />
          </div>
          <p className="muted small">{t("homeHint")}</p>
          <p className="actions">
            <button type="button" className="button primary" disabled={busy} onClick={() => void setHome(true)}>
              {article.on_home ? t("homeUpdate") : t("homeOn")}
            </button>
            {article.on_home && (
              <button type="button" className="button" disabled={busy} onClick={() => void setHome(false)}>
                {t("homeTurnOff")}
              </button>
            )}
          </p>
        </div>
      ) : (
        <p className="muted small">{t("homeNeedsPublish")}</p>
      )}
    </section>
  );
}

export function ArticleView({ slug }: { slug: string }) {
  const t = useTranslations("articles");
  const f = useFormatter();
  const { isModerator } = useSession();
  const [article, setArticle] = useState<Article>();
  const [error, setError] = useState<unknown>();
  const load = useCallback(async () => {
    try {
      setArticle(await call(api.GET("/api/v1/articles/{key}", { params: { path: { key: slug } } })));
      setError(undefined);
    } catch (e) {
      setError(e);
    }
  }, [slug]);
  useEffect(() => {
    void load();
  }, [load, isModerator]);
  if (error !== undefined) return <ErrorState error={error} onRetry={load} />;
  if (!article) return <LoadingState />;
  return (
    <article className="article">
      {article.cover_image_id && (
        // eslint-disable-next-line @next/next/no-img-element
        <img className="article-cover" {...articleCover(article.cover_image_id)} sizes="(min-width: 900px) 860px, 100vw" alt="" decoding="async" />
      )}
      <header>
        <h1 dir="auto">{article.title}</h1>
        {article.summary && (
          <p className="lead" dir="auto">
            {article.summary}
          </p>
        )}
        <p className="muted small">
          {article.published_at ? f.dateTime(new Date(article.published_at), { dateStyle: "long" }) : <span className="badge">{t("draft")}</span>}
          {isModerator && (
            <>
              {" "}
              <Link href={`/articles/${article.slug}/edit`} className="button">
                <Pencil size={14} aria-hidden="true" /> {t("edit")}
              </Link>
            </>
          )}
        </p>
      </header>
      <ArticleBody markdown={article.body_md} />
      {isModerator && <HomeControls key={article.version} article={article} onChange={setArticle} />}
    </article>
  );
}
