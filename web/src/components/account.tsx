"use client";

import { useCallback, useEffect, useState } from "react";
import { useFormatter, useTranslations } from "next-intl";
import { api, ApiProblem, call, type User } from "@/api/client";
import { Link, useRouter } from "@/i18n/navigation";
import { useSubmit } from "./form";
import { EmptyState, ErrorState, LoadingState, ProblemAlert } from "./states";

type Load = { status: "loading" } | { status: "signed-out" } | { status: "error"; error: unknown } | { status: "ready"; user: User };

/** Protected view: loads the current user from the session cookie. */
export function AccountView() {
  const t = useTranslations("account");
  const tNav = useTranslations("nav");
  const format = useFormatter();
  const router = useRouter();
  const [load, setLoad] = useState<Load>({ status: "loading" });

  const fetchMe = useCallback(async () => {
    setLoad({ status: "loading" });
    try {
      setLoad({ status: "ready", user: await call(api.GET("/api/v1/me")) });
    } catch (error) {
      if (error instanceof ApiProblem && error.status === 401) setLoad({ status: "signed-out" });
      else setLoad({ status: "error", error });
    }
  }, []);

  useEffect(() => {
    void fetchMe();
  }, [fetchMe]);

  const logout = useSubmit(async () => {
    await call(api.POST("/api/v1/auth/logout"));
    router.push("/login");
  });

  switch (load.status) {
    case "loading":
      return <LoadingState />;
    case "error":
      return <ErrorState error={load.error} onRetry={fetchMe} />;
    case "signed-out":
      return (
        <EmptyState title={t("signedOutTitle")}>
          <p>{t("signedOutBody")}</p>
          <Link className="button primary" href="/login">
            {tNav("signIn")}
          </Link>
        </EmptyState>
      );
    case "ready": {
      const u = load.user;
      return (
        <section aria-labelledby="account-heading">
          <dl className="details">
            <dt>{t("display_name")}</dt>
            {/* Personal names render verbatim in their own direction; never translated. */}
            <dd dir="auto" translate="no">
              {u.display_name}
            </dd>
            <dt>{t("email")}</dt>
            <dd dir="ltr">{u.email}</dd>
            <dt>{t("roles")}</dt>
            <dd>{u.roles.length ? format.list(u.roles.map((r) => t(`role_${r}`))) : t("noRoles")}</dd>
            <dt>{t("memberSince")}</dt>
            <dd>{format.dateTime(new Date(u.created_at), { dateStyle: "long" })}</dd>
          </dl>
          {logout.error !== undefined && <ProblemAlert error={logout.error} />}
          <button className="button" type="button" onClick={() => logout.submit()} disabled={logout.submitting}>
            {t("signOut")}
          </button>
        </section>
      );
    }
  }
}
