"use client";

import { Pencil, X } from "lucide-react";
import { useTranslations } from "next-intl";
import { useEffect, useRef, useState } from "react";
import { api, call } from "@/api/client";
import { Link } from "@/i18n/navigation";
import { ProblemAlert } from "../states";
import { useSession } from "../session";

/**
 * "Found a mistake?" on each lyric. A moderator edits it directly; anyone else signed in sends the corrected
 * text, which waits in the moderators' queue and changes nothing until it is accepted.
 */
export function CorrectionButton({ id, body, language, dir, name }: { id: string; body: string; language: string; dir: string; name: string }) {
  const t = useTranslations("public.panes");
  const { user, isModerator } = useSession();
  const [open, setOpen] = useState(false);
  if (!user && user !== null) return null;
  if (isModerator)
    return (
      <Link href={`/contributions/${id}/edit`} className="icon-button" aria-label={t("edit", { name })} title={t("edit", { name })}>
        <Pencil size={16} aria-hidden="true" />
      </Link>
    );
  if (user === null)
    return (
      <Link href="/login" className="icon-button" aria-label={t("signInToCorrect")} title={t("signInToCorrect")}>
        <Pencil size={16} aria-hidden="true" />
      </Link>
    );
  return (
    <>
      <button type="button" className="icon-button" aria-label={t("suggest", { name })} title={t("suggest", { name })} onClick={() => setOpen(true)}>
        <Pencil size={16} aria-hidden="true" />
      </button>
      {open && <CorrectionDialog id={id} body={body} language={language} dir={dir} onClose={() => setOpen(false)} />}
    </>
  );
}

function CorrectionDialog({ id, body, language, dir, onClose }: { id: string; body: string; language: string; dir: string; onClose: () => void }) {
  const t = useTranslations("public.panes");
  const [text, setText] = useState(body);
  const [note, setNote] = useState("");
  const [error, setError] = useState<unknown>(undefined);
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    area.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);
  const send = async () => {
    setBusy(true);
    setError(undefined);
    try {
      await call(api.POST("/api/v1/contributions/{id}/corrections", { params: { path: { id } }, body: { body: text, note: note.trim() || null } }));
      setSent(true);
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-backdrop">
      <div className="modal correction" role="dialog" aria-modal="true" aria-label={t("suggestTitle")}>
        <header className="modal-head">
          <h2>{t("suggestTitle")}</h2>
          <button type="button" className="icon-button" aria-label={t("cancel")} onClick={onClose}>
            <X size={16} aria-hidden="true" />
          </button>
        </header>
        {sent ? (
          <>
            <p role="status">{t("suggestDone")}</p>
            <p className="actions">
              <button type="button" className="button primary" autoFocus onClick={onClose}>
                {t("ok")}
              </button>
            </p>
          </>
        ) : (
          <form
            noValidate
            onSubmit={(e) => {
              e.preventDefault();
              void send();
            }}
          >
            <p className="muted small">{t("suggestHint")}</p>
            {error !== undefined && <ProblemAlert error={error} />}
            <div className="field">
              <label>
                {t("suggestText")}
                <textarea ref={area} rows={14} value={text} lang={language} dir={dir} onChange={(e) => setText(e.target.value)} />
              </label>
            </div>
            <div className="field">
              <label>
                {t("suggestNote")}
                <input type="text" value={note} maxLength={500} onChange={(e) => setNote(e.target.value)} />
              </label>
            </div>
            <p className="actions">
              <button type="submit" className="button primary" disabled={busy || text === body} aria-busy={busy}>
                {busy ? t("suggestSending") : t("suggestSend")}
              </button>
              <button type="button" className="button" onClick={onClose}>
                {t("cancel")}
              </button>
            </p>
          </form>
        )}
      </div>
    </div>
  );
}
