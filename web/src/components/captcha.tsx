"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useLocale, useTranslations } from "next-intl";
import { api, call } from "@/api/client";

type Turnstile = {
  render: (el: HTMLElement, o: Record<string, unknown>) => string;
  reset: (id?: string) => void;
  remove: (id?: string) => void;
};
declare global {
  interface Window {
    turnstile?: Turnstile;
  }
}

const SCRIPT = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

let config: Promise<{ enabled: boolean; site_key?: string | null }> | undefined;
/** Whether forms need the bot check, asked once per page load. A failure counts as "off": the server decides. */
const loadConfig = () => (config ??= call(api.GET("/api/v1/captcha")).catch(() => ({ enabled: false })));

let script: Promise<void> | undefined;
function loadScript(): Promise<void> {
  script ??= new Promise<void>((resolve, reject) => {
    if (window.turnstile) return resolve();
    const s = document.createElement("script");
    s.src = SCRIPT;
    s.async = true;
    s.onload = () => resolve();
    s.onerror = () => {
      script = undefined;
      reject(new Error("turnstile"));
    };
    document.head.appendChild(s);
  });
  return script;
}

/**
 * The bot check (Cloudflare Turnstile), only when the site has keys set. `widget` goes inside the form;
 * `blocked` is true while a challenge is shown and not yet answered; `take()` returns the answer for the
 * request and asks for a fresh challenge, because an answer works once.
 */
export function useCaptcha() {
  const [siteKey, setSiteKey] = useState<string>();
  const [token, setToken] = useState<string>();
  const [failed, setFailed] = useState(false);
  const widgetId = useRef<string | undefined>(undefined);
  const box = useRef<HTMLDivElement>(null);
  const locale = useLocale();

  useEffect(() => {
    let live = true;
    void loadConfig().then((c) => live && c.enabled && c.site_key && setSiteKey(c.site_key));
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    if (!siteKey) return;
    let live = true;
    loadScript().then(
      () => {
        if (!live || !box.current || !window.turnstile) return;
        widgetId.current = window.turnstile.render(box.current, {
          sitekey: siteKey,
          language: locale,
          callback: (t: string) => setToken(t),
          "expired-callback": () => setToken(undefined),
          "error-callback": () => setToken(undefined),
        });
      },
      () => live && setFailed(true),
    );
    return () => {
      live = false;
      if (widgetId.current) window.turnstile?.remove(widgetId.current);
      widgetId.current = undefined;
    };
  }, [siteKey, locale]);

  const take = useCallback(() => {
    const t = token;
    setToken(undefined);
    if (widgetId.current) window.turnstile?.reset(widgetId.current);
    return t;
  }, [token]);

  return { enabled: !!siteKey, blocked: !!siteKey && !token, failed, take, box };
}

/** The place where the challenge is drawn. */
export function CaptchaWidget({ captcha, fieldErrors = [] }: { captcha: ReturnType<typeof useCaptcha>; fieldErrors?: { field: string; code: string }[] }) {
  const t = useTranslations("captcha");
  const tv = useTranslations("validation");
  if (!captcha.enabled) return null;
  const err = fieldErrors.find((e) => e.field === "captcha");
  return (
    <div className="field captcha">
      <div ref={captcha.box} />
      {captcha.failed && <p className="alert alert-warning">{t("unavailable")}</p>}
      {err && (
        <p className="field-error" role="alert">
          {tv(err.code as "captcha_failed")}
        </p>
      )}
    </div>
  );
}
