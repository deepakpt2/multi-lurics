"use client";

import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { Eye, MessageCircle, Sparkles, Star } from "lucide-react";
import { api, call } from "@/api/client";
import { useSession } from "../session";

type Stats = { views?: number; ratings: number; average: number | null; comments: number };

/**
 * What the page has earned so far: views (this visit is counted once per day), the average rating and
 * the comments on its lyrics. Loaded after the page, so the page itself stays cacheable.
 */
export function PageStats({ kind, id, lyricIds }: { kind: "song" | "artist"; id: string; lyricIds: string[] }) {
  const t = useTranslations("public.stats");
  const { isModerator } = useSession();
  const th = useTranslations("home");
  const [picked, setPicked] = useState<boolean>();
  const [stats, setStats] = useState<Stats>();
  useEffect(() => {
    if (kind !== "song" || !isModerator) return;
    call(api.GET("/api/v1/editors-picks/{id}", { params: { path: { id } } })).then((r) => setPicked(r.picked), () => undefined);
  }, [kind, id, isModerator]);
  const togglePick = () =>
    call(picked ? api.DELETE("/api/v1/editors-picks/{id}", { params: { path: { id } } }) : api.PUT("/api/v1/editors-picks/{id}", { params: { path: { id } } })).then(
      (r) => setPicked(r.picked),
      () => undefined,
    );
  const key = lyricIds.join(",");
  useEffect(() => {
    let live = true;
    (async () => {
      const [views, engagement] = await Promise.all([
        call(api.POST("/api/v1/views", { body: { kind, id } })).then((v) => v.views, () => undefined),
        Promise.all(
          key
            .split(",")
            .filter(Boolean)
            .slice(0, 6)
            .map((c) => call(api.GET("/api/v1/contributions/{id}/engagement", { params: { path: { id: c } } })).catch(() => null)),
        ),
      ]);
      const rated = engagement.filter((e) => e !== null);
      const ratings = rated.reduce((n, e) => n + e.rating_count, 0);
      const sum = rated.reduce((n, e) => n + (e.rating_average ?? 0) * e.rating_count, 0);
      if (live) setStats({ views, ratings, average: ratings ? sum / ratings : null, comments: rated.reduce((n, e) => n + e.comment_count, 0) });
    })();
    return () => {
      live = false;
    };
  }, [kind, id, key]);
  if (!stats) return null;
  return (
    <p className="page-stats muted small" aria-label={t("label")}>
      {stats.views !== undefined && (
        <span>
          <Eye size={14} aria-hidden="true" /> {t("views", { count: stats.views })}
        </span>
      )}
      {kind === "song" && (
        <>
          <span>
            <Star size={14} aria-hidden="true" />{" "}
            {stats.average !== null ? t("rating", { average: stats.average.toFixed(1), count: stats.ratings }) : t("noRatings")}
          </span>
          <span>
            <MessageCircle size={14} aria-hidden="true" /> {t("comments", { count: stats.comments })}
          </span>
        </>
      )}
      {kind === "song" && isModerator && picked !== undefined && (
        <button type="button" className="button" aria-pressed={picked} onClick={togglePick}>
          <Sparkles size={14} aria-hidden="true" /> {picked ? th("picked") : th("pick")}
        </button>
      )}
    </p>
  );
}
