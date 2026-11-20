"use client";

import { useState } from "react";
import { Ban, CircleCheck, ShieldCheck, ShieldOff, UserCog } from "lucide-react";
import { useFormatter, useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { usePaged } from "../catalogue/common";
import { useSession } from "../session";
import { EmptyState, LoadingState, ProblemAlert } from "../states";

type AdminUser = components["schemas"]["AdminUser"];

function UserRow({ u, self, onChange }: { u: AdminUser; self: boolean; onChange: (u: AdminUser) => void }) {
  const t = useTranslations("admin");
  const format = useFormatter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>();
  const run = async (action: () => Promise<AdminUser>) => {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      onChange(await action());
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  const role = (name: "moderator" | "admin", grant: boolean) =>
    run(() => {
      const params = { params: { path: { id: u.id, role: name } } };
      return call(grant ? api.PUT("/api/v1/admin/users/{id}/roles/{role}", params) : api.DELETE("/api/v1/admin/users/{id}/roles/{role}", params));
    });
  const status = (disable: boolean) =>
    run(() => call(api.POST(disable ? "/api/v1/admin/users/{id}/disable" : "/api/v1/admin/users/{id}/enable", { params: { path: { id: u.id } } })));
  const has = (r: string) => u.roles.includes(r);
  return (
    <li className="user-card" data-testid="user-row">
      <div className="user-head">
        <div>
          <strong dir="auto" translate="no">{u.display_name}</strong>
          {self && <span className="badge">{t("you")}</span>}
          <p className="muted small" dir="ltr">{u.email}</p>
        </div>
        <p className="chips">
          <span className={`badge status-${u.status}`}>{t(`status.${u.status as "active"}`)}</span>
          {u.roles.map((r) => (
            <span key={r} className="badge role">{t(`roles.${r as "admin"}`)}</span>
          ))}
        </p>
      </div>
      <p className="muted small">{t("joined", { date: format.dateTime(new Date(u.created_at), { dateStyle: "medium" }) })}</p>
      {error !== undefined && <ProblemAlert error={error} />}
      {!self && (
        <p className="actions">
          <button type="button" className="button" disabled={busy} onClick={() => role("moderator", !has("moderator"))}>
            {has("moderator") ? <ShieldOff size={16} aria-hidden="true" /> : <ShieldCheck size={16} aria-hidden="true" />}
            {has("moderator") ? t("removeModerator") : t("makeModerator")}
          </button>
          <button type="button" className="button" disabled={busy} onClick={() => role("admin", !has("admin"))}>
            <UserCog size={16} aria-hidden="true" />
            {has("admin") ? t("removeAdmin") : t("makeAdmin")}
          </button>
          <button type="button" className="button" disabled={busy} onClick={() => status(u.status !== "disabled")}>
            {u.status === "disabled" ? <CircleCheck size={16} aria-hidden="true" /> : <Ban size={16} aria-hidden="true" />}
            {u.status === "disabled" ? t("enable") : t("disable")}
          </button>
        </p>
      )}
    </li>
  );
}

export function UsersAdmin() {
  const t = useTranslations("admin");
  const { user: me } = useSession();
  const [q, setQ] = useState("");
  const [applied, setApplied] = useState("");
  const list = usePaged<AdminUser>(`users:${applied}`, (cursor) =>
    call(api.GET("/api/v1/admin/users", { params: { query: { q: applied || undefined, cursor } } })),
  );
  const [edited, setEdited] = useState<Record<string, AdminUser>>({});
  return (
    <section>
      <p className="lead">{t("usersIntro")}</p>
      <form role="search" className="search-form" onSubmit={(e) => (e.preventDefault(), setApplied(q.trim()))}>
        <label className="visually-hidden" htmlFor="user-q">{t("search")}</label>
        <input id="user-q" type="search" value={q} placeholder={t("search")} dir="auto" onChange={(e) => setQ(e.target.value)} />
        <button type="submit" className="button primary">{t("searchButton")}</button>
      </form>
      {list.status === "loading" && <LoadingState />}
      {list.status === "error" && <ProblemAlert error={list.error} onReload={list.retry} />}
      {list.status !== "loading" && list.status !== "error" && list.items.length === 0 && <EmptyState title={t("noUsers")} />}
      <ul className="queue">
        {list.items.map((u) => (
          <UserRow key={u.id} u={edited[u.id] ?? u} self={me?.id === u.id} onChange={(n) => setEdited((m) => ({ ...m, [n.id]: n }))} />
        ))}
      </ul>
      {list.hasMore && (
        <button type="button" className="button" onClick={list.loadMore} disabled={list.status === "more"}>
          {t("loadMore")}
        </button>
      )}
    </section>
  );
}
