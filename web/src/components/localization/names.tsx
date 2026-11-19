"use client";

import { useState } from "react";
import { useLocale, useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { components } from "@/api/schema";
import { Link } from "@/i18n/navigation";
import { Field, formValues, useFieldError, useSubmit } from "../form";
import { Loaded, useLoad } from "../catalogue/common";
import { ProblemAlert } from "../states";
import { languageName } from "./localized";

type S = components["schemas"];
export type EntityType = "composition" | "recording" | "artist" | "album" | "label" | "publisher" | "genre" | "credit_role";
export const PROVENANCES = ["original", "human_translation", "transliteration", "community", "imported", "machine"] as const satisfies readonly S["Provenance"][];

export const listLanguages = () => call(api.GET("/api/v1/languages", { params: { query: { limit: 100 } } }));
export const listScripts = () => call(api.GET("/api/v1/scripts", { params: { query: { limit: 100 } } }));

export const languageLabel = (l: S["Language"]) => `${l.endonym} (${l.name_en})`;
export const scriptLabel = (x: S["Script"]) => `${x.iso15924} ${x.name_en}`;

type FieldErrors = { field: string; code: string }[];

export function SelectBox({ name, label, value, onChange, children, fieldErrors }: {
  name: string;
  label: string;
  value: string;
  onChange: (v: string) => void;
  children: React.ReactNode;
  fieldErrors: FieldErrors;
}) {
  const err = useFieldError(fieldErrors, name);
  return (
    <div className="field">
      <label>
        {label}
        <select name={name} value={value} onChange={(e) => onChange(e.target.value)} aria-invalid={err ? true : undefined}>
          {children}
        </select>
      </label>
      {err && <p className="field-error">{err}</p>}
    </div>
  );
}

/**
 * Language-aware form: the name field takes `lang`/`dir` from the chosen language or script, so
 * right-to-left names are typed right-to-left and shaped by the right font.
 */
export function LocalizationForm({ entityType, entityId, onSaved }: { entityType: EntityType; entityId: string; onSaved: () => void }) {
  const t = useTranslations("localization");
  const tf = useTranslations("fields");
  const ui = useTranslations();
  const refs = useLoad("refs", async () => ({ languages: (await listLanguages()).items, scripts: (await listScripts()).items }));
  const [language, setLanguage] = useState("");
  const [script, setScript] = useState("");
  const [provenance, setProvenance] = useState<S["Provenance"]>("original");
  const s = useSubmit(async (v: Record<string, string>) => {
    const created = await call(
      api.POST("/api/v1/entities/{entity_type}/{entity_id}/localizations", {
        params: { path: { entity_type: entityType, entity_id: entityId } },
        body: {
          language,
          script: script || null,
          name: v.name,
          description: v.description?.trim() || null,
          provenance,
          source: v.source?.trim() || null,
        },
      }),
    );
    setPublished(created.verification_state === "verified");
    onSaved();
  });
  const [published, setPublished] = useState(false);
  return (
    <Loaded load={refs}>
      {({ languages, scripts }) => {
        const lang = languages.find((l) => l.iso639_3 === language);
        const scr = scripts.find((x) => x.iso15924 === script);
        const rtl = scr?.is_rtl ?? lang?.is_rtl ?? false;
        const tag = lang ? (lang.iso639_1 ?? lang.iso639_3) : undefined;
        return (
          <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))} aria-label={t("addTitle")}>
            {s.error !== undefined && <ProblemAlert error={s.error} />}
            {s.state.status === "success" && (
              <div className="alert alert-success" role="status" data-state="success">
                <p>{published ? t("published") : t("submitted")}</p>
              </div>
            )}
            <SelectBox name="language" label={tf("language")} value={language} onChange={setLanguage} fieldErrors={s.fieldErrors}>
              <option value="">{t("choose")}</option>
              {languages.map((l) => (
                <option key={l.id} value={l.iso639_3}>
                  {languageLabel(l)}
                </option>
              ))}
            </SelectBox>
            <SelectBox name="script" label={tf("script")} value={script} onChange={setScript} fieldErrors={s.fieldErrors}>
              <option value="">{t("autoScript")}</option>
              {scripts.map((x) => (
                <option key={x.id} value={x.iso15924}>
                  {scriptLabel(x)}
                </option>
              ))}
            </SelectBox>
            <Field name="name" dir={rtl ? "rtl" : "ltr"} lang={tag} verbatim required fieldErrors={s.fieldErrors} />
            <Field name="description" dir={rtl ? "rtl" : "ltr"} lang={tag} verbatim fieldErrors={s.fieldErrors} />
            <SelectBox
              name="provenance"
              label={tf("provenance")}
              value={provenance}
              onChange={(v) => setProvenance(v as S["Provenance"])}
              fieldErrors={s.fieldErrors}
            >
              {PROVENANCES.filter((p) => !(entityType === "artist" && p === "machine")).map((p) => (
                <option key={p} value={p}>
                  {ui(`localization.provenance.${p}`)}
                </option>
              ))}
            </SelectBox>
            <Field name="source" dir="auto" hint={entityType === "artist" ? t("sourceHintArtist") : t("sourceHint")} fieldErrors={s.fieldErrors} />
            <button className="button primary" type="submit" disabled={s.submitting || !language} aria-busy={s.submitting}>
              {s.submitting ? ui("common.submitting") : ui("catalogue.save")}
            </button>
          </form>
        );
      }}
    </Loaded>
  );
}

function AliasForm({ entityType, entityId, onSaved }: { entityType: EntityType; entityId: string; onSaved: () => void }) {
  const t = useTranslations("localization");
  const ui = useTranslations();
  const s = useSubmit(async (v: Record<string, string>) => {
    await call(
      api.POST("/api/v1/entities/{entity_type}/{entity_id}/aliases", {
        params: { path: { entity_type: entityType, entity_id: entityId } },
        body: { alias: v.alias, source: v.source },
      }),
    );
    onSaved();
  });
  return (
    <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))} aria-label={t("addAliasTitle")}>
      {s.error !== undefined && <ProblemAlert error={s.error} />}
      {s.state.status === "success" && (
        <div className="alert alert-success" role="status" data-state="success">
          <p>{t("submitted")}</p>
        </div>
      )}
      <Field name="alias" dir="auto" verbatim required fieldErrors={s.fieldErrors} />
      <Field name="source" dir="auto" required hint={t("aliasSourceHint")} fieldErrors={s.fieldErrors} />
      <button className="button" type="submit" disabled={s.submitting} aria-busy={s.submitting}>
        {s.submitting ? ui("common.submitting") : ui("catalogue.save")}
      </button>
    </form>
  );
}

/** Verified names and aliases of one entity, plus forms to propose more. */
export function NamesView({ entityType, entityId }: { entityType: EntityType; entityId: string }) {
  const t = useTranslations("localization");
  const uiLocale = useLocale();
  const load = useLoad(`${entityType}/${entityId}`, async () => {
    const path = { entity_type: entityType, entity_id: entityId };
    const [names, aliases] = await Promise.all([
      call(api.GET("/api/v1/entities/{entity_type}/{entity_id}/localizations", { params: { path, query: { limit: 100 } } })),
      call(api.GET("/api/v1/entities/{entity_type}/{entity_id}/aliases", { params: { path, query: { limit: 100 } } })),
    ]);
    return { names: names.items, aliases: aliases.items };
  });
  return (
    <>
      <Loaded load={load}>
        {({ names, aliases }) => (
          <>
            <h2>{t("namesHeading")}</h2>
            {names.length === 0 ? (
              <p className="muted">{t("none")}</p>
            ) : (
              <ul className="entity-list">
                {names.map((n) => (
                  <li key={n.id}>
                    <span lang={n.language} dir={n.dir} translate="no">
                      {n.name}
                    </span>
                    <span className="muted small">
                      {" "}
                      {languageName(n.script ? `${n.language}-${n.script}` : n.language, uiLocale)}
                      {n.is_preferred ? t("preferredSuffix") : ""}
                    </span>
                    {n.source && <span className="muted small"> {t("sourceLabel", { source: n.source })}</span>}
                  </li>
                ))}
              </ul>
            )}
            <h2>{t("aliasesHeading")}</h2>
            {aliases.length === 0 ? (
              <p className="muted">{t("noAliases")}</p>
            ) : (
              <ul className="entity-list">
                {aliases.map((a) => (
                  <li key={a.id}>
                    <span lang={a.language ?? undefined} dir={a.dir} translate="no">
                      {a.alias}
                    </span>
                    <span className="muted small"> {t("sourceLabel", { source: a.source })}</span>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
      </Loaded>
      <h2>{t("addTitle")}</h2>
      <LocalizationForm entityType={entityType} entityId={entityId} onSaved={load.reload} />
      <h2>{t("addAliasTitle")}</h2>
      <AliasForm entityType={entityType} entityId={entityId} onSaved={load.reload} />
    </>
  );
}

export function NamesLink({ entityType, entityId }: { entityType: EntityType; entityId: string }) {
  const t = useTranslations("localization");
  return (
    <Link className="button" href={`/names/${entityType}/${entityId}`}>
      {t("linkLabel")}
    </Link>
  );
}
