"use client";

import { useTranslations } from "next-intl";
import { useState } from "react";
import { api, call } from "@/api/client";
import type { Block } from "@/lib/contribute";
import { LyricsDialog } from "./lyrics-dialog";

/**
 * Adds one set of lyrics (a translation, a transliteration or another original) to a song that is already
 * in the catalogue. Afterwards it says whether the lyrics are live or waiting for a moderator.
 */
export function AddLyricsDialog({ songId, title, type, onClose }: { songId: string; title?: string; type?: Block["type"]; onClose: () => void }) {
  const t = useTranslations("contribute");
  const [status, setStatus] = useState<string | null>(null);
  if (status)
    return (
      <div className="modal-backdrop">
        <div className="modal" role="dialog" aria-modal="true" aria-label={t("done.title")}>
          <h2>{t("done.title")}</h2>
          <p role="status">{status === "published" ? t("done.lyricsPublished") : t("done.lyricsPending")}</p>
          <p className="actions">
            <button type="button" className="button primary" autoFocus onClick={onClose}>
              {t("done.ok")}
            </button>
          </p>
        </div>
      </div>
    );
  return (
    <LyricsDialog
      title={title}
      initial={{ type }}
      onClose={onClose}
      onSave={async (b) => {
        const r = await call(api.POST("/api/v1/compositions/{id}/lyrics", { params: { path: { id: songId } }, body: b }));
        setStatus(r.status);
      }}
    />
  );
}
