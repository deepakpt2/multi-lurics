"use client";

import { useTranslations } from "next-intl";
import { useState } from "react";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Loaded, useLoad } from "../catalogue/common";
import { EmptyState, ProblemAlert } from "../states";

type Item = components["schemas"]["CorrectionItem"];

function Row({ k, onDone }: { k: Item; onDone: () => void }) {
  const t = useTranslations("corrections");
  const [error, setError] = useState<unknown>(undefined);
  const [busy, setBusy] = useState(false);
  const decide = async (verb: "apply" | "reject") => {
    setBusy(true);
    setError(undefined);
    try {
      const path = { id: k.id };
      if (verb === "apply") await call(api.POST("/api/v1/moderation/corrections/{id}/apply", { params: { path }, body: {} }));
      else await call(api.POST("/api/v1/moderation/corrections/{id}/reject", { params: { path }, body: {} }));
      onDone();
    } catch (e) {
      setError(e);
      setBusy(false);
    }
  };
  return (
    <li className="queue-item">
      {k.note && <p>{t("reason", { note: k.note })}</p>}
      {error !== undefined && <ProblemAlert error={error} />}
      <div className="correction-compare">
        <section>
          <h3>{t("current")}</h3>
          <pre className="lyric-body" lang={k.language}>{k.current_body}</pre>
        </section>
        <section>
          <h3>{t("suggested")}</h3>
          <pre className="lyric-body" lang={k.language}>{k.body}</pre>
        </section>
      </div>
      <p className="actions">
        <button type="button" className="button primary" disabled={busy} onClick={() => decide("apply")}>
          {t("apply")}
        </button>
        <button type="button" className="button" disabled={busy} onClick={() => decide("reject")}>
          {t("reject")}
        </button>
      </p>
    </li>
  );
}

/** Corrections readers have suggested to published lyrics, oldest first. */
export function CorrectionsQueue() {
  const t = useTranslations("corrections");
  const load = useLoad("corrections", async () => (await call(api.GET("/api/v1/moderation/corrections"))).items);
  return (
    <Loaded load={load}>
      {(items) =>
        items.length === 0 ? (
          <EmptyState title={t("empty")} />
        ) : (
          <ul className="queue">
            {items.map((k) => (
              <Row key={k.id} k={k} onDone={load.reload} />
            ))}
          </ul>
        )
      }
    </Loaded>
  );
}
