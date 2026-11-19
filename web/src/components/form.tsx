"use client";

import { useId, useState } from "react";
import { useTranslations } from "next-intl";
import { ApiProblem } from "@/api/client";

type SubmitState = { status: "idle" | "submitting" | "success" } | { status: "error"; error: unknown };

/** The single form-submission pattern: one in-flight request, problem captured for display. */
export function useSubmit<A extends unknown[]>(action: (...args: A) => Promise<unknown>) {
  const [state, setState] = useState<SubmitState>({ status: "idle" });
  const submit = async (...args: A) => {
    if (state.status === "submitting") return;
    setState({ status: "submitting" });
    try {
      await action(...args);
      setState({ status: "success" });
    } catch (error) {
      setState({ status: "error", error });
    }
  };
  const error = state.status === "error" ? state.error : undefined;
  const fieldErrors = error instanceof ApiProblem ? error.fieldErrors : [];
  return { state, submit, error, fieldErrors, submitting: state.status === "submitting" };
}

type FieldProps = {
  name: string;
  type?: "text" | "email" | "password";
  autoComplete?: string;
  hint?: string;
  fieldErrors: { field: string; code: string }[];
  dir?: "auto" | "ltr" | "rtl";
};

/** Labelled input wired to server-side field errors (localized by code). */
export function Field({ name, type = "text", autoComplete, hint, fieldErrors, dir }: FieldProps) {
  const t = useTranslations();
  const id = useId();
  const err = fieldErrors.find((e) => e.field === name);
  const errKey = err && t.has(`validation.${err.code}`) ? `validation.${err.code}` : "validation.generic";
  const describedBy = [hint && `${id}-hint`, err && `${id}-err`].filter(Boolean).join(" ") || undefined;
  return (
    <div className="field">
      <label htmlFor={id}>{t(`fields.${name}`)}</label>
      <input
        id={id}
        name={name}
        type={type}
        autoComplete={autoComplete}
        dir={dir}
        aria-invalid={err ? true : undefined}
        aria-describedby={describedBy}
      />
      {hint && (
        <p id={`${id}-hint`} className="muted small">
          {hint}
        </p>
      )}
      {err && (
        <p id={`${id}-err`} className="field-error">
          {t(errKey)}
        </p>
      )}
    </div>
  );
}

export function formValues(form: HTMLFormElement): Record<string, string> {
  return Object.fromEntries([...new FormData(form).entries()].map(([k, v]) => [k, String(v)]));
}

/** Single-use tokens arrive in the URL fragment (never sent to servers or Referer). */
export function readFragmentToken(): string | null {
  if (typeof window === "undefined") return null;
  return new URLSearchParams(window.location.hash.slice(1)).get("token");
}
