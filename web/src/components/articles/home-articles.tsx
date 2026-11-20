"use client";

import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { ArrowRight } from "lucide-react";
import { api, call } from "@/api/client";
import { Link } from "@/i18n/navigation";
import { articleCover, type Article } from "@/lib/articles";

/**
 * Special-occasion articles a moderator has put on the home page. Takes no room when there are none:
 * the space exists only while an article's period is running.
 */
export function HomeArticles() {
  const t = useTranslations("articles");
  const [items, setItems] = useState<Article[]>([]);
  useEffect(() => {
    let live = true;
    call(api.GET("/api/v1/articles/home")).then(
      (r) => live && setItems(r.items),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);
  if (items.length === 0) return null;
  return (
    <section className="home-articles" aria-label={t("homeLabel")}>
      {items.map((a) => (
        <article key={a.id} className="home-article">
          {a.cover_image_id && (
            // eslint-disable-next-line @next/next/no-img-element
            <img {...articleCover(a.cover_image_id)} sizes="(min-width: 900px) 480px, 100vw" alt="" decoding="async" />
          )}
          <div className="home-article-text">
            <h2 dir="auto">{a.title}</h2>
            {a.summary && <p dir="auto">{a.summary}</p>}
            <Link className="button primary" href={`/articles/${a.slug}`}>
              {t("read")} <ArrowRight size={16} aria-hidden="true" />
            </Link>
          </div>
        </article>
      ))}
    </section>
  );
}
