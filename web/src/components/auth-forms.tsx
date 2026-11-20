"use client";

import { useEffect, useRef, useState } from "react";
import { useLocale, useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import { Link, useRouter } from "@/i18n/navigation";
import { Field, formValues, readFragmentToken, useSubmit } from "./form";
import { LoadingState, ProblemAlert } from "./states";

/** Mirrors the server's policy for the hint only; the server is the authority. */
export const PASSWORD_MIN = 12;

function Success({ title, children }: { title?: string; children: React.ReactNode }) {
  return (
    <div className="alert alert-success" role="status" data-state="success">
      {title && <strong>{title}</strong>}
      <p>{children}</p>
    </div>
  );
}

const GOOGLE_ERRORS = ["google_failed", "google_denied", "google_disabled", "google_email_unverified"];

/** "Continue with Google", shown only when the server has it configured. Full-page navigation: the flow is redirect-based. */
export function SocialSignIn() {
  const t = useTranslations();
  const locale = useLocale();
  const [google, setGoogle] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const e = new URLSearchParams(window.location.search).get("error");
    if (e !== null) setError(GOOGLE_ERRORS.includes(e) ? e : "google_failed");
    void api.GET("/api/v1/auth/providers").then((r) => setGoogle(r.data?.google === true));
  }, []);
  return (
    <>
      {error !== null && (
        <div className="alert alert-error" role="alert" data-state="error">
          <p>{t(`google.errors.${error}`)}</p>
        </div>
      )}
      {google && (
        <p>
          <a className="button" href={`/api/v1/auth/google/start?locale=${locale}`}>
            {t("google.continue")}
          </a>
        </p>
      )}
    </>
  );
}

export function RegisterForm() {
  const t = useTranslations();
  const locale = useLocale();
  const s = useSubmit((v: Record<string, string>) =>
    call(
      api.POST("/api/v1/auth/register", {
        body: { email: v.email, display_name: v.display_name, password: v.password, locale },
      }),
    ),
  );
  if (s.state.status === "success") return <Success title={t("register.successTitle")}>{t("register.success")}</Success>;
  return (
    <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))}>
      <SocialSignIn />
      {s.error !== undefined && <ProblemAlert error={s.error} />}
      <Field name="email" type="email" autoComplete="email" dir="ltr" fieldErrors={s.fieldErrors} />
      <Field name="display_name" autoComplete="nickname" dir="auto" hint={t("hints.display_name")} fieldErrors={s.fieldErrors} />
      <Field
        name="password"
        type="password"
        autoComplete="new-password"
        hint={t("hints.password", { min: PASSWORD_MIN })}
        fieldErrors={s.fieldErrors}
      />
      <button className="button primary" type="submit" disabled={s.submitting} aria-busy={s.submitting}>
        {s.submitting ? t("common.submitting") : t("register.submit")}
      </button>
      <p>
        <Link href="/login">{t("register.haveAccount")}</Link>
      </p>
    </form>
  );
}

export function LoginForm() {
  const t = useTranslations();
  const router = useRouter();
  const s = useSubmit(async (v: Record<string, string>) => {
    await call(api.POST("/api/v1/auth/login", { body: { email: v.email, password: v.password } }));
    router.push("/account");
  });
  return (
    <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))}>
      <SocialSignIn />
      {s.error !== undefined && <ProblemAlert error={s.error} />}
      <Field name="email" type="email" autoComplete="email" dir="ltr" fieldErrors={s.fieldErrors} />
      <Field name="password" type="password" autoComplete="current-password" fieldErrors={s.fieldErrors} />
      <button className="button primary" type="submit" disabled={s.submitting} aria-busy={s.submitting}>
        {s.submitting ? t("common.submitting") : t("login.submit")}
      </button>
      <p>
        <Link href="/forgot-password">{t("login.forgot")}</Link>
      </p>
      <p>
        <Link href="/register">{t("login.noAccount")}</Link>
      </p>
    </form>
  );
}

export function ForgotPasswordForm() {
  const t = useTranslations();
  const locale = useLocale();
  const s = useSubmit((v: Record<string, string>) =>
    call(api.POST("/api/v1/auth/password-reset", { body: { email: v.email, locale } })),
  );
  if (s.state.status === "success") return <Success title={t("forgot.successTitle")}>{t("forgot.success")}</Success>;
  return (
    <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))}>
      <p>{t("forgot.intro")}</p>
      {s.error !== undefined && <ProblemAlert error={s.error} />}
      <Field name="email" type="email" autoComplete="email" dir="ltr" fieldErrors={s.fieldErrors} />
      <button className="button primary" type="submit" disabled={s.submitting} aria-busy={s.submitting}>
        {s.submitting ? t("common.submitting") : t("forgot.submit")}
      </button>
    </form>
  );
}

export function ResetPasswordForm() {
  const t = useTranslations();
  const [token, setToken] = useState<string | null | undefined>(undefined);
  useEffect(() => setToken(readFragmentToken()), []);
  const s = useSubmit((v: Record<string, string>) =>
    call(api.POST("/api/v1/auth/password-reset/confirm", { body: { token: token ?? "", new_password: v.new_password } })),
  );
  if (token === undefined) return <LoadingState />;
  if (token === null)
    return (
      <div className="alert alert-error" role="alert">
        <p>{t("reset.missingToken")}</p>
        <Link href="/forgot-password">{t("forgot.title")}</Link>
      </div>
    );
  if (s.state.status === "success")
    return (
      <Success>
        {t("reset.success")} <Link href="/login">{t("login.submit")}</Link>
      </Success>
    );
  return (
    <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))}>
      {s.error !== undefined && <ProblemAlert error={s.error} />}
      <Field
        name="new_password"
        type="password"
        autoComplete="new-password"
        hint={t("hints.password", { min: PASSWORD_MIN })}
        fieldErrors={s.fieldErrors}
      />
      <button className="button primary" type="submit" disabled={s.submitting} aria-busy={s.submitting}>
        {s.submitting ? t("common.submitting") : t("reset.submit")}
      </button>
    </form>
  );
}

export function VerifyEmail() {
  const t = useTranslations();
  const [missing, setMissing] = useState(false);
  const started = useRef(false);
  const s = useSubmit((token: string) => call(api.POST("/api/v1/auth/verify-email", { body: { token } })));
  useEffect(() => {
    if (started.current) return; // tokens are single-use: never submit twice (StrictMode)
    started.current = true;
    const token = readFragmentToken();
    if (token) void s.submit(token);
    else setMissing(true);
  }, [s]);
  if (missing)
    return (
      <div className="alert alert-error" role="alert">
        <p>{t("verify.missingToken")}</p>
      </div>
    );
  if (s.state.status === "success")
    return (
      <Success>
        {t("verify.success")} <Link href="/login">{t("verify.continue")}</Link>
      </Success>
    );
  if (s.error !== undefined) return <ProblemAlert error={s.error} />;
  return <LoadingState label={t("verify.verifying")} />;
}
