"use client";

import { useEffect, useId, useState } from "react";
import { useTranslations } from "next-intl";
import { ApiProblem } from "@/api/client";
import { formatBytes, loadLimits, uploadImage, type Image, type ImageEntity, type ImageLimits } from "@/lib/images";
import { ProblemAlert } from "../states";
import { useFieldError } from "../form";

const ACCEPT = "image/jpeg,image/png,image/webp,image/gif";

/**
 * Choose a picture, say it may be shared, optionally credit it, and send it. Not a `<form>`, so it
 * can sit inside another form (the song and proposal forms). Where the picture goes is decided by
 * the server: with `entity` it waits for review (or is published for moderators and trusted
 * contributors); without, it is held as an unsent draft to attach to a new-entity proposal.
 */
export function ImageUpload({
  entity,
  onUploaded,
  onCancel,
  submitLabel,
}: {
  entity?: { type: ImageEntity; id: string };
  onUploaded: (image: Image) => void;
  onCancel?: () => void;
  submitLabel?: string;
}) {
  const t = useTranslations("images");
  const id = useId();
  const [limits, setLimits] = useState<ImageLimits>();
  const [file, setFile] = useState<File>();
  const [preview, setPreview] = useState<string>();
  const [rights, setRights] = useState(false);
  const [credit, setCredit] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>();
  const [tooBig, setTooBig] = useState(false);

  useEffect(() => {
    loadLimits().then(setLimits, () => undefined);
  }, []);
  useEffect(() => {
    if (!file || typeof URL.createObjectURL !== "function") return setPreview(undefined);
    const url = URL.createObjectURL(file);
    setPreview(url);
    return () => URL.revokeObjectURL(url);
  }, [file]);

  const fieldErrors = error instanceof ApiProblem ? error.fieldErrors : [];
  const fileError = useFieldError(fieldErrors, "file");
  const rightsError = useFieldError(fieldErrors, "rights");

  const send = async () => {
    if (!file || !rights || busy) return;
    setBusy(true);
    setError(undefined);
    try {
      onUploaded(await uploadImage(file, { rights, credit, entity }));
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="image-upload">
      {error !== undefined && fieldErrors.length === 0 && <ProblemAlert error={error} />}
      <div className="field">
        <label htmlFor={`${id}-file`}>{t("file")}</label>
        <input
          id={`${id}-file`}
          type="file"
          accept={ACCEPT}
          aria-describedby={`${id}-limits`}
          aria-invalid={!!fileError || tooBig}
          onChange={(e) => {
            const f = e.target.files?.[0];
            setError(undefined);
            setTooBig(!!f && !!limits && f.size > limits.max_bytes);
            setFile(f);
          }}
        />
        {limits && (
          <p id={`${id}-limits`} className="muted small">
            {t("limits", { max: formatBytes(limits.max_bytes), min: limits.min_side })}
          </p>
        )}
        {tooBig && limits && (
          <p role="alert" className="field-error">
            {t("tooBig", { max: formatBytes(limits.max_bytes) })}
          </p>
        )}
        {fileError && (
          <p role="alert" className="field-error">
            {fileError}
          </p>
        )}
      </div>
      {preview && !tooBig && (
        // A local preview of the chosen file; nothing is sent until the person confirms.
        // eslint-disable-next-line @next/next/no-img-element
        <img className="image-upload-preview" src={preview} alt="" width={96} height={96} />
      )}
      <div className="field">
        <label htmlFor={`${id}-credit`}>{t("credit")}</label>
        <input id={`${id}-credit`} value={credit} maxLength={300} dir="auto" onChange={(e) => setCredit(e.target.value)} onKeyDown={(e) => e.key === "Enter" && e.preventDefault()} />
        <p className="muted small">{t("creditHint")}</p>
      </div>
      <div className="field">
        <label className="check">
          <input type="checkbox" checked={rights} onChange={(e) => setRights(e.target.checked)} aria-invalid={!!rightsError} />
          <span>{t("rights")}</span>
        </label>
        {rightsError && (
          <p role="alert" className="field-error">
            {rightsError}
          </p>
        )}
      </div>
      <p className="actions">
        <button type="button" className="button primary" disabled={!file || !rights || tooBig || busy} aria-busy={busy} onClick={send}>
          {busy ? t("uploading") : (submitLabel ?? t("upload"))}
        </button>
        {onCancel && (
          <button type="button" className="button" onClick={onCancel}>
            {t("cancel")}
          </button>
        )}
      </p>
    </div>
  );
}
