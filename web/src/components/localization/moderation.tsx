"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Link } from "@/i18n/navigation";
import { usePaged } from "../catalogue/common";
import { EmptyState, ErrorState, LoadingState, ProblemAlert } from "../states";
import { useSubmit } from "../form";

type Loc = components["schemas"]["Localization"];

const PATHS: Record<string, string> = {
  composition: "compositions",
  recording: "recordings",
  artist: "artists",
  album: "albums",
};

function QueueItem({ item, onDone }: { item: Loc; onDone: (id: string) => void }) {
  const t = useTranslations("localization");
  const decide = useSubmit(async (verb: "verify" | "reject") => {
    const path = { id: item.id };
    const body = { expected_parent_revision_id: item.current_revision_id };
    await call(
      verb === "verify"
        ? api.POST("/api/v1/localizations/{id}/verify", { params: { path }, body })
        : api.POST("/api/v1/localizations/{id}/reject", { params: { path }, body }),
    );
    onDone(item.id);
  });
  const section = PATHS[item.entity_type];
  const tag = item.script ? `${item.language}-${item.script}` : item.language;
  const meta = [t(`entityTypes.${item.entity_type}`), tag, t(`provenance.${item.provenance}`)];
  if (item.source) meta.push(t("sourceLabel", { source: item.source }));
  return (
    <li className="queue-item">
      <p>
        <span lang={item.language} dir={item.dir} translate="no">
          {item.name}
        </span>
      </p>
      <p className="muted small">{meta.join(t("separator"))}</p>
      {section && (
        <p>
          <Link href={`/${section}/${item.entity_id}`}>{t("openEntity")}</Link>
        </p>
      )}
      {decide.error !== undefined && <ProblemAlert error={decide.error} onReload={() => onDone(item.id)} />}
      <p className="actions">
        <button className="button primary" type="button" disabled={decide.submitting} onClick={() => decide.submit("verify")}>
          {t("approve")}
        </button>
        <button className="button" type="button" disabled={decide.submitting} onClick={() => decide.submit("reject")}>
          {t("reject")}
        </button>
      </p>
    </li>
  );
}

/** Pending metadata awaiting a moderator. A stale decision shows the conflict and reloads the queue. */
export function ModerationQueue() {
  const t = useTranslations("localization");
  const list = usePaged<Loc>("pending", (cursor) => call(api.GET("/api/v1/moderation/localizations", { params: { query: { cursor } } })));
  const [done, setDone] = useState<Set<string>>(new Set());
  if (list.status === "loading") return <LoadingState />;
  if (list.status === "error" && list.items.length === 0) return <ErrorState error={list.error} onRetry={list.retry} />;
  const items = list.items.filter((i) => !done.has(i.id));
  if (items.length === 0 && !list.hasMore) return <EmptyState title={t("queueEmpty")} />;
  return (
    <>
      <ul className="queue">
        {items.map((i) => (
          <QueueItem key={i.id} item={i} onDone={(id) => setDone((d) => new Set(d).add(id))} />
        ))}
      </ul>
      {list.hasMore && (
        <button className="button" type="button" onClick={list.loadMore}>
          {t("loadMore")}
        </button>
      )}
    </>
  );
}
