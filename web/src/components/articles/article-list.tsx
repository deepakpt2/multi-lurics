"use client";

import { useEffect, useState } from "react";
import { useFormatter, useTranslations } from "next-intl";
import { Plus } from "lucide-react";
import { api, call } from "@/api/client";
import { Link } from "@/i18n/navigation";
import { articleCover, type Article } from "@/lib/articles";
import { useSession } from "../session";
import { EmptyState, ErrorState, LoadingState } from "../states";

export function ArticleCard({ a, showStatus }: { a: Article; showStatus?: boolean }) {
  const t = useTranslations("articles");
  const f = useFormatter();
  return (
    <article className="article-card">
      <Link href={`/articles/${a.slug}`} className="article-card-link">
        {a.cover_image_id && (
          // eslint-disable-next-line @next/next/no-img-element
          <img {...articleCover(a.cover_image_id)} sizes="(min-width: 720px) 320px, 100vw" alt="" loading="lazy" decoding="async" />
        )}
        <span className="article-card-text">
          <strong dir="auto">{a.title}</strong>
          {a.summary && (
            <span className="muted" dir="auto">
              {a.summary}
            </span>
          )}
          <span className="muted small">
            {a.published_at ? f.dateTime(new Date(a.published_at), { dateStyle: "medium" }) : null}
            {showStatus && a.status === "draft" && <span className="badge">{t("draft")}</span>}
            {showStatus && a.home_now && <span className="badge badge-primary">{t("onHomeNow")}</span>}
          </span>
        </span>
      </Link>
    </article>
  );
}

export function ArticleList() {
  const t = useTranslations("articles");
  const { isModerator } = useSession();
  const [items, setItems] = useState<Article[]>();
  const [error, setError] = useState<unknown>();
  const [tries, setTries] = useState(0);
  useEffect(() => {
    let live = true;
    call(api.GET("/api/v1/articles")).then(
      (r) => live && (setItems(r.items), setError(undefined)),
      (e) => live && setError(e),
    );
    return () => {
      live = false;
    };
  }, [tries, isModerator]);
  return (
    <>
      {isModerator && (
        <p className="actions">
          <Link href="/articles/new" className="button primary">
            <Plus size={16} aria-hidden="true" /> {t("new")}
          </Link>
        </p>
      )}
      {error !== undefined ? (
        <ErrorState error={error} onRetry={() => setTries((n) => n + 1)} />
      ) : !items ? (
        <LoadingState />
      ) : items.length === 0 ? (
        <EmptyState title={t("none")} />
      ) : (
        <div className="article-grid">
          {items.map((a) => (
            <ArticleCard key={a.id} a={a} showStatus={isModerator} />
          ))}
        </div>
      )}
    </>
  );
}
