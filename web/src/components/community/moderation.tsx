"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { usePaged } from "../catalogue/common";
import { EmptyState, LoadingState, ProblemAlert } from "../states";
import { Link } from "@/i18n/navigation";

type S = components["schemas"];

function ReportRow({ r, onDone }: { r: S["Report"]; onDone: () => void }) {
  const t = useTranslations("community");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const [error, setError] = useState<unknown>();
  const decide = async (resolution: "upheld" | "dismissed", action?: "hide_comment") => {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      await call(api.POST("/api/v1/moderation/reports/{id}/resolve", { params: { path: { id: r.id } }, body: { resolution, action, note: note.trim() || null } }));
      onDone();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <li className="proposal" data-testid="report">
      <p>
        <span className="badge">{t(`targets.${r.target_type as "comment"}`)}</span>{" "}
        <span className="badge">{t(`reasons.${r.reason as "spam"}`)}</span>{" "}
        {r.target_type === "contribution" && <Link href={`/contributions/${r.target_id}`}>{t("open")}</Link>}
      </p>
      {r.detail && <p dir="auto" className="muted">{r.detail}</p>}
      <div className="field">
        <label htmlFor={`note-${r.id}`}>{t("moderatorNote")}</label>
        <input id={`note-${r.id}`} value={note} dir="auto" onChange={(e) => setNote(e.target.value)} />
      </div>
      {error !== undefined && <ProblemAlert error={error} />}
      <p className="actions">
        <button type="button" className="button primary" disabled={busy} onClick={() => decide("upheld")}>{t("uphold")}</button>
        {r.target_type === "comment" && (
          <button type="button" className="button" disabled={busy} onClick={() => decide("upheld", "hide_comment")}>{t("upholdHide")}</button>
        )}
        <button type="button" className="button" disabled={busy} onClick={() => decide("dismissed")}>{t("dismiss")}</button>
      </p>
    </li>
  );
}

export function ReportQueue() {
  const t = useTranslations("community");
  const list = usePaged<S["Report"]>("reports", (cursor) => call(api.GET("/api/v1/moderation/reports", { params: { query: { cursor } } })));
  if (list.status === "loading") return <LoadingState />;
  if (list.status === "error") return <ProblemAlert error={list.error} onReload={list.retry} />;
  if (list.items.length === 0) return <EmptyState title={t("noReports")} />;
  return (
    <>
      <ul className="proposals">
        {list.items.map((r) => (
          <ReportRow key={r.id} r={r} onDone={list.retry} />
        ))}
      </ul>
      {list.hasMore && <button type="button" className="button" onClick={list.loadMore}>{t("more")}</button>}
    </>
  );
}

function AppealRow({ a, onDone }: { a: S["Appeal"]; onDone: () => void }) {
  const t = useTranslations("community");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>();
  const decide = async (decision: "upheld" | "denied") => {
    setBusy(true);
    setError(undefined);
    try {
      await call(api.POST("/api/v1/moderation/appeals/{id}/decide", { params: { path: { id: a.id } }, body: { decision } }));
      onDone();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <li className="proposal" data-testid="appeal">
      <p dir="auto">{a.message}</p>
      {error !== undefined && <ProblemAlert error={error} />}
      <p className="actions">
        <button type="button" className="button primary" disabled={busy} onClick={() => decide("upheld")}>{t("restore")}</button>
        <button type="button" className="button" disabled={busy} onClick={() => decide("denied")}>{t("deny")}</button>
      </p>
    </li>
  );
}

export function AppealQueue() {
  const t = useTranslations("community");
  const list = usePaged<S["Appeal"]>("appeals", (cursor) => call(api.GET("/api/v1/moderation/appeals", { params: { query: { cursor } } })));
  if (list.status === "loading") return <LoadingState />;
  if (list.status === "error") return <ProblemAlert error={list.error} onReload={list.retry} />;
  if (list.items.length === 0) return <EmptyState title={t("noAppeals")} />;
  return (
    <ul className="proposals">
      {list.items.map((a) => (
        <AppealRow key={a.id} a={a} onDone={list.retry} />
      ))}
    </ul>
  );
}
