"use client";

import { useId, useState } from "react";
import { useSearchParams } from "next/navigation";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { usePaged } from "../catalogue/common";
import { CaptchaWidget, useCaptcha } from "../captcha";
import { useFieldError, useSubmit } from "../form";
import { EmptyState, LoadingState, ProblemAlert } from "../states";
import { Link } from "@/i18n/navigation";

type S = components["schemas"];
const KINDS = ["copyright", "content", "privacy"] as const;

function FieldMessage({ id, errors, name }: { id: string; errors: { field: string; code: string }[]; name: string }) {
  const msg = useFieldError(errors, name);
  return msg ? (
    <p id={id} className="field-error" role="alert">
      {msg}
    </p>
  ) : null;
}

/** Public form: no account needed. The receipt carries only a reference, never the contact details. */
export function NoticeForm() {
  const t = useTranslations("legal");
  const tf = useTranslations("fields");
  const uid = useId();
  const initial = useSearchParams().get("contribution") ?? "";
  const [kind, setKind] = useState<(typeof KINDS)[number]>("copyright");
  const [receipt, setReceipt] = useState<S["NoticeReceipt"]>();
  const captcha = useCaptcha();
  const { submit, error, fieldErrors, submitting } = useSubmit(async (fd: FormData) => {
    const s = (k: string) => String(fd.get(k) ?? "").trim();
    setReceipt(
      await call(
        api.POST("/api/v1/legal/notices", {
          body: {
            kind,
            contribution_id: kind === "privacy" ? null : s("contribution_id") || null,
            contact_name: s("contact_name"),
            contact_email: s("contact_email"),
            statement: s("statement"),
            good_faith: fd.get("good_faith") === "on",
            captcha_token: captcha.take(),
          },
        }),
      ),
    );
  });
  if (receipt)
    return (
      <section role="status" aria-labelledby={`${uid}-r`}>
        <h2 id={`${uid}-r`}>{t("receiptTitle")}</h2>
        <p>{t("receipt", { reference: receipt.reference })}</p>
      </section>
    );
  const row = (name: string, input: React.ReactNode) => (
    <div className="field">
      <label htmlFor={`${uid}-${name}`}>{tf(name as "email")}</label>
      {input}
      <FieldMessage id={`${uid}-${name}-err`} errors={fieldErrors} name={name} />
    </div>
  );
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        void submit(new FormData(e.currentTarget));
      }}
    >
      <div className="field">
        <label htmlFor={`${uid}-kind`}>{t("kind")}</label>
        <select id={`${uid}-kind`} value={kind} onChange={(e) => setKind(e.target.value as (typeof KINDS)[number])}>
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {t(`kinds.${k}`)}
            </option>
          ))}
        </select>
      </div>
      {kind !== "privacy" && (
        <>
          {row("contribution_id", <input id={`${uid}-contribution_id`} name="contribution_id" defaultValue={initial} dir="ltr" autoComplete="off" aria-describedby={`${uid}-hint`} />)}
          <p id={`${uid}-hint`} className="muted small">
            {t("contributionHint")}
          </p>
        </>
      )}
      {row("contact_name", <input id={`${uid}-contact_name`} name="contact_name" autoComplete="name" dir="auto" required />)}
      {row("contact_email", <input id={`${uid}-contact_email`} name="contact_email" type="email" autoComplete="email" dir="ltr" required />)}
      {row("statement", <textarea id={`${uid}-statement`} name="statement" rows={6} dir="auto" required />)}
      <div className="field">
        <label>
          <input type="checkbox" name="good_faith" /> {t("goodFaith")}
        </label>
        <FieldMessage id={`${uid}-gf-err`} errors={fieldErrors} name="good_faith" />
      </div>
      <CaptchaWidget captcha={captcha} fieldErrors={fieldErrors} />
      {error !== undefined && fieldErrors.length === 0 && <ProblemAlert error={error} />}
      <button type="submit" className="button primary" disabled={submitting || captcha.blocked}>
        {submitting ? t("sending") : t("send")}
      </button>
    </form>
  );
}

function NoticeRow({ n, onDone }: { n: S["Notice"]; onDone: () => void }) {
  const t = useTranslations("legal");
  const uid = useId();
  const [note, setNote] = useState("");
  const [error, setError] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  const decide = async (action: "takedown" | "reject" | "close") => {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      await call(api.POST("/api/v1/moderation/legal/notices/{id}/decide", { params: { path: { id: n.id } }, body: { action, note: note.trim() || null } }));
      onDone();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <li className="proposal" data-testid="notice">
      <p>
        <span className="badge">{t(`kindsShort.${n.kind as "copyright"}`)}</span> <span className="badge">{t(`statuses.${n.status as "open"}`)}</span>{" "}
        <span dir="ltr">{n.reference}</span>{" "}
        {n.contribution_id && <Link href={`/contributions/${n.contribution_id}`}>{t("open")}</Link>}
      </p>
      <p className="muted">
        <bdi>{t("from", { name: n.contact_name, email: n.contact_email })}</bdi>
      </p>
      <p dir="auto">{n.statement}</p>
      {n.status === "open" && (
        <>
          <div className="field">
            <label htmlFor={`${uid}-note`}>{t("note")}</label>
            <input id={`${uid}-note`} value={note} dir="auto" onChange={(e) => setNote(e.target.value)} />
          </div>
          {error !== undefined && <ProblemAlert error={error} />}
          <p className="actions">
            {n.kind !== "privacy" && (
              <button type="button" className="button primary" disabled={busy} onClick={() => decide("takedown")}>
                {t("takedown")}
              </button>
            )}
            <button type="button" className="button" disabled={busy} onClick={() => decide("reject")}>
              {t("reject")}
            </button>
            <button type="button" className="button" disabled={busy} onClick={() => decide("close")}>
              {t("close")}
            </button>
          </p>
        </>
      )}
    </li>
  );
}

export function NoticeQueue() {
  const t = useTranslations("legal");
  const tm = useTranslations("catalogue");
  const list = usePaged<S["Notice"]>("legal-notices", (cursor) => call(api.GET("/api/v1/moderation/legal/notices", { params: { query: { cursor } } })));
  if (list.status === "loading") return <LoadingState />;
  if (list.status === "error") return <ProblemAlert error={list.error} onReload={list.retry} />;
  if (list.items.length === 0) return <EmptyState title={t("noNotices")} />;
  return (
    <>
      <ul className="proposals">
        {list.items.map((n) => (
          <NoticeRow key={n.id} n={n} onDone={list.retry} />
        ))}
      </ul>
      {list.hasMore && (
        <button type="button" className="button" onClick={list.loadMore}>
          {tm("loadMore")}
        </button>
      )}
    </>
  );
}
