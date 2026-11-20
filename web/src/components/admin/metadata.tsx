"use client";

import { useTranslations } from "next-intl";
import { useState } from "react";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { useSession } from "../session";
import { useSubmit } from "../form";
import { ProblemAlert } from "../states";

type Result = components["schemas"]["MetadataResult"];
const KINDS = ["artist", "album", "raga", "deity", "tag"] as const;
type Kind = (typeof KINDS)[number];
const SUB: Partial<Record<Kind, readonly string[]>> = {
  artist: ["person", "group", "other"],
  album: ["album", "soundtrack", "single", "ep", "compilation", "other"],
};

/**
 * Admins add singers, albums, ragas, deities and tags before contributors need them, so the suggestions
 * on the contribution form offer them and nobody makes a second copy. Names that exist are left alone.
 */
export function MetadataAdmin() {
  const t = useTranslations("admin.metadata");
  const tc = useTranslations("catalogue");
  const { user, isAdmin } = useSession();
  const [kind, setKind] = useState<Kind>("artist");
  const [sub, setSub] = useState("");
  const [year, setYear] = useState("");
  const [names, setNames] = useState("");
  const [results, setResults] = useState<Result[] | null>(null);
  const s = useSubmit(async () => {
    const list = names.split("\n").map((n) => n.trim()).filter(Boolean);
    const y = Number.parseInt(year, 10);
    const r = await call(
      api.POST("/api/v1/admin/metadata", { body: { kind, names: list, sub_kind: sub || null, year: kind === "album" && Number.isFinite(y) ? y : null } }),
    );
    setResults(r.results);
    // What was created is gone from the box; what could not be made stays to be fixed.
    setNames(r.results.filter((x) => x.problem).map((x) => x.name).join("\n"));
  });
  if (user === undefined) return null;
  if (!isAdmin) return <p className="alert alert-warning">{t("adminOnly")}</p>;
  const made = results?.filter((r) => r.created) ?? [];
  const existing = results?.filter((r) => !r.created && !r.problem) ?? [];
  const failed = results?.filter((r) => r.problem) ?? [];
  const subs = SUB[kind];
  return (
    <form
      noValidate
      className="contribute-form"
      onSubmit={(e) => {
        e.preventDefault();
        void s.submit();
      }}
    >
      {s.error !== undefined && <ProblemAlert error={s.error} />}
      <div className="field" role="radiogroup" aria-label={t("kind")}>
        <span className="field-label">{t("kind")}</span>
        <div className="segmented">
          {KINDS.map((k) => (
            <button
              key={k}
              type="button"
              role="radio"
              aria-checked={kind === k}
              onClick={() => {
                setKind(k);
                setSub("");
              }}
            >
              {t(`kinds.${k}`)}
            </button>
          ))}
        </div>
      </div>
      {subs && (
        <div className="field">
          <label htmlFor="md-sub">{t("subKind")}</label>
          <select id="md-sub" value={sub} onChange={(e) => setSub(e.target.value)}>
            <option value="">{subs[0] === "person" ? tc("artistKinds.person") : tc("albumKinds.album")}</option>
            {subs.slice(1).map((x) => (
              <option key={x} value={x}>
                {kind === "artist" ? tc(`artistKinds.${x as "group"}`) : tc(`albumKinds.${x as "ep"}`)}
              </option>
            ))}
          </select>
        </div>
      )}
      {kind === "album" && (
        <div className="field">
          <label htmlFor="md-year">{t("year")}</label>
          <input id="md-year" type="number" inputMode="numeric" value={year} onChange={(e) => setYear(e.target.value)} />
        </div>
      )}
      <div className="field">
        <label htmlFor="md-names">{t("names")}</label>
        <textarea id="md-names" rows={10} dir="auto" value={names} onChange={(e) => setNames(e.target.value)} />
        <p className="muted small">{t("namesHint")}</p>
      </div>
      <p className="actions">
        <button className="button primary" type="submit" disabled={s.submitting || !names.trim()} aria-busy={s.submitting}>
          {s.submitting ? t("working") : t("create")}
        </button>
      </p>
      {results && (
        <section className="import-result" role="status">
          <p>{t("summary", { created: made.length, existing: existing.length, failed: failed.length })}</p>
          {[
            ["made", made],
            ["existing", existing],
            ["failed", failed],
          ].map(([label, list]) =>
            (list as Result[]).length > 0 ? (
              <details key={label as string} open={label !== "existing"}>
                <summary>
                  {t(label as "made")} <span className="muted">{(list as Result[]).length}</span>
                </summary>
                <ul>
                  {(list as Result[]).map((r, i) => (
                    <li key={`${r.name}-${i}`} dir="auto">
                      {r.name}
                    </li>
                  ))}
                </ul>
              </details>
            ) : null,
          )}
        </section>
      )}
    </form>
  );
}
