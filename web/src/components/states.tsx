"use client";

import { useTranslations } from "next-intl";
import { ApiProblem } from "@/api/client";

export function LoadingState({ label }: { label?: string }) {
  const t = useTranslations("common");
  return (
    <div className="state state-loading" role="status" aria-live="polite" aria-busy="true">
      <span className="spinner" aria-hidden="true" />
      <span>{label ?? t("loading")}</span>
    </div>
  );
}

export function EmptyState({ title, children }: { title?: string; children?: React.ReactNode }) {
  const t = useTranslations("common");
  return (
    <div className="state state-empty" role="status">
      <p>{title ?? t("empty")}</p>
      {children}
    </div>
  );
}

/** Localized message for any error. Unknown codes fall back to a generic message. */
export function useProblemMessage() {
  const t = useTranslations("errors");
  return (error: unknown) => {
    const code = error instanceof ApiProblem ? error.code : "generic";
    return t.has(code) ? t(code) : t("generic");
  };
}

/** Optimistic-concurrency conflict (409). Offers to load the latest version; never auto-merges. */
export function ConflictNotice({ onReload }: { onReload?: () => void }) {
  const t = useTranslations("conflict");
  return (
    <div className="alert alert-warning" role="alert" data-state="conflict">
      <strong>{t("title")}</strong>
      <p>{t("body")}</p>
      {onReload && (
        <button type="button" className="button" onClick={onReload}>
          {t("reload")}
        </button>
      )}
    </div>
  );
}

/** Inline error for forms and actions. */
export function ProblemAlert({ error, onReload }: { error: unknown; onReload?: () => void }) {
  const message = useProblemMessage();
  const t = useTranslations("errors");
  if (error instanceof ApiProblem && error.isConflict) return <ConflictNotice onReload={onReload} />;
  const requestId = error instanceof ApiProblem ? error.problem.request_id : undefined;
  return (
    <div className="alert alert-error" role="alert" data-state="error">
      <p>{message(error)}</p>
      {requestId && <p className="muted small">{t("requestId", { id: requestId })}</p>}
    </div>
  );
}

/** Full-region error with retry, for failed loads. */
export function ErrorState({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const t = useTranslations();
  return (
    <div className="state state-error">
      <h2>{t("errors.title")}</h2>
      <ProblemAlert error={error} onReload={onRetry} />
      {onRetry && !(error instanceof ApiProblem && error.isConflict) && (
        <button type="button" className="button" onClick={onRetry}>
          {t("common.retry")}
        </button>
      )}
    </div>
  );
}
