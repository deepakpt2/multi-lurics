"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { ApiProblem, api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { usePaged } from "../catalogue/common";
import { EmptyState, LoadingState, ProblemAlert } from "../states";
import { Name } from "../localization/localized";
import { mediaSources } from "@/lib/images";
import { suggest, type Suggestion } from "./entity-picker";

type Proposal = components["schemas"]["Proposal"];

function Row({ p, onDone }: { p: Proposal; onDone: () => void }) {
  const t = useTranslations("entities");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>();
  const [likely, setLikely] = useState<Suggestion[]>();

  const decide = async (verb: "approve" | "reject", confirm = false) => {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      await call(
        api.POST(verb === "approve" ? "/api/v1/entities/proposals/{id}/approve" : "/api/v1/entities/proposals/{id}/reject", {
          params: { path: { id: p.id } },
          body: { confirm_distinct: confirm },
        }),
      );
      onDone();
    } catch (e) {
      if (e instanceof ApiProblem && e.code === "likely_duplicates") {
        try {
          setLikely(await suggest(p.entity_type as Parameters<typeof suggest>[0], p.name));
        } catch (inner) {
          setError(inner);
        }
      } else {
        setError(e);
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className="proposal">
      <p>
        <strong dir="auto" translate="no">
          {p.name}
        </strong>{" "}
        <span className="muted small">{t(`types.${p.entity_type as "artist"}`)}</span>
      </p>
      {p.image_id && (
        // eslint-disable-next-line @next/next/no-img-element
        <img {...mediaSources(p.image_id, 96)} alt="" width={96} height={96} style={{ borderRadius: 12 }} />
      )}
      {p.disambiguation && (
        <p className="muted small" dir="auto">
          {p.disambiguation}
        </p>
      )}
      {error !== undefined && <ProblemAlert error={error} />}
      {likely && (
        <div className="alert alert-warning" role="alert">
          <p>{t("moderatorLikely")}</p>
          <ul className="entity-options">
            {likely.map((s) => (
              <li key={s.id} className="entity-option">
                <Name display={s.display} />
              </li>
            ))}
          </ul>
          <button type="button" className="button" disabled={busy} onClick={() => decide("approve", true)}>
            {t("approveAnyway")}
          </button>
        </div>
      )}
      <p className="actions">
        <button type="button" className="button primary" disabled={busy} onClick={() => decide("approve")}>
          {t("approve")}
        </button>
        <button type="button" className="button" disabled={busy} onClick={() => decide("reject")}>
          {t("reject")}
        </button>
      </p>
    </li>
  );
}

export function ProposalQueue() {
  const t = useTranslations("entities");
  const list = usePaged<Proposal>("proposals", (cursor) =>
    call(api.GET("/api/v1/moderation/entity-proposals", { params: { query: { cursor } } })),
  );
  if (list.status === "loading" && list.items.length === 0) return <LoadingState />;
  if (list.status === "error") return <ProblemAlert error={list.error} />;
  if (list.items.length === 0) return <EmptyState title={t("queueEmpty")} />;
  return (
    <>
      <ul className="proposals">
        {list.items.map((p) => (
          <Row key={p.id} p={p} onDone={list.retry} />
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
