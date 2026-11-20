"use client";

import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { Name } from "../localization/localized";
import { DEBOUNCE_MS, suggest, type EntityType, type Suggestion } from "../entities/entity-picker";

/**
 * Under a free-text raga, deity or tag field: shows the existing record the typed text will be
 * linked to when the song is saved, in whatever script it was typed. Purely informational;
 * a failed lookup shows nothing because saving still links the text.
 */
export function MatchHint({ type, value }: { type: EntityType; value: string }) {
  const t = useTranslations("catalogue.match");
  const q = value.trim();
  const [found, setFound] = useState<{ q: string; item: Suggestion } | null>(null);

  useEffect(() => {
    if (q.length < 2) return;
    const ctl = new AbortController();
    const timer = setTimeout(() => {
      suggest(type, q, ctl.signal)
        .then((items) => {
          if (ctl.signal.aborted) return;
          const hit = items.find((i) => i.score >= 0.95);
          setFound(hit ? { q, item: hit } : null);
        })
        .catch(() => undefined);
    }, DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      ctl.abort();
    };
  }, [q, type]);

  if (q.length < 2 || found?.q !== q) return null;
  return (
    <p className="hint" role="status">
      {t("linked")} <Name display={found.item.display} />
    </p>
  );
}
