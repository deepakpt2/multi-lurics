"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Link } from "@/i18n/navigation";
import { mediaSources } from "@/lib/images";
import { usePaged } from "../catalogue/common";
import { EmptyState, LoadingState, ProblemAlert } from "../states";

type Review = components["schemas"]["ImageReview"];

function Row({ r, onDone }: { r: Review; onDone: () => void }) {
  const t = useTranslations("images");
  const te = useTranslations("entities");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>();
  const [rejecting, setRejecting] = useState(false);
  const [note, setNote] = useState("");
  const type = r.image.entity_type;

  const decide = async (verb: "approve" | "reject") => {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      await call(
        api.POST(verb === "approve" ? "/api/v1/images/{id}/approve" : "/api/v1/images/{id}/reject", {
          params: { path: { id: r.image.id } },
          body: { note: note.trim() || null },
        }),
      );
      onDone();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className="review-image">
      <p>
        <strong dir="auto" translate="no">
          {r.subject}
        </strong>{" "}
        {type && <span className="muted small">{te(`types.${type as "artist"}`)}</span>}
      </p>
      <div className="review-pair">
        <figure>
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img {...mediaSources(r.image.id, 160)} alt={t("proposedAlt", { name: r.subject })} width={160} height={160} />
          <figcaption className="muted small">{t("proposed")}</figcaption>
        </figure>
        {r.current_id && (
          <figure>
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img {...mediaSources(r.current_id, 160)} alt={t("currentAlt", { name: r.subject })} width={160} height={160} />
            <figcaption className="muted small">{t("current")}</figcaption>
          </figure>
        )}
      </div>
      <p className="muted small">
        <span>{t("uploadedBy", { name: r.uploader })}</span>{" "}
        {r.image.credit && (
          <span>
            {t("creditLine")} <span dir="auto">{r.image.credit}</span>
          </span>
        )}{" "}
        <span>{t("dimensions", { width: r.image.width, height: r.image.height })}</span>
      </p>
      {error !== undefined && <ProblemAlert error={error} />}
      {r.proposal_id ? (
        <p>
          {t("withProposal")} <Link href="/moderation/entities">{t("openProposals")}</Link>
        </p>
      ) : rejecting ? (
        <div className="field">
          <label htmlFor={`note-${r.image.id}`}>{t("rejectReason")}</label>
          <input id={`note-${r.image.id}`} value={note} maxLength={1000} onChange={(e) => setNote(e.target.value)} />
          <p className="actions">
            <button type="button" className="button primary" disabled={busy || !note.trim()} onClick={() => decide("reject")}>
              {t("confirmReject")}
            </button>
            <button type="button" className="button" onClick={() => setRejecting(false)}>
              {t("cancel")}
            </button>
          </p>
        </div>
      ) : (
        <p className="actions">
          <button type="button" className="button primary" disabled={busy} onClick={() => decide("approve")}>
            {t("approve")}
          </button>
          <button type="button" className="button" disabled={busy} onClick={() => setRejecting(true)}>
            {t("reject")}
          </button>
        </p>
      )}
    </li>
  );
}

export function ImageQueue() {
  const t = useTranslations("images");
  const list = usePaged<Review>("images", (cursor) => call(api.GET("/api/v1/moderation/images", { params: { query: { cursor } } })));
  if (list.status === "loading" && list.items.length === 0) return <LoadingState />;
  if (list.status === "error") return <ProblemAlert error={list.error} />;
  if (list.items.length === 0) return <EmptyState title={t("queueEmpty")} />;
  return (
    <>
      <ul className="review-images">
        {list.items.map((r) => (
          <Row key={r.image.id} r={r} onDone={list.retry} />
        ))}
      </ul>
      {list.hasMore && (
        <button type="button" className="button" onClick={list.loadMore}>
          {t("loadMore")}
        </button>
      )}
    </>
  );
}
