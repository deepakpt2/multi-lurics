"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import { Link, useRouter } from "@/i18n/navigation";
import { Name } from "../localization/localized";
import { NamesLink } from "../localization/names";
import { Field, formValues, useSubmit } from "../form";
import { CREDIT_ROLES, CreditList, FormActions, ListShell, Loaded, RefListEditor, SaveProblem, int, str, text, useLoad, usePaged, type S } from "./common";
import { searchArtists } from "./search";

type Query = Record<string, string>;
type Role = S["CreditRole"];

export function CompositionList({ query }: { query: Query }) {
  const [filter, setFilter] = useState({ prefix: query.prefix ?? "", artist_id: query.artist_id ?? "" });
  const list = usePaged<S["Composition"]>(JSON.stringify(filter), (cursor) =>
    call(
      api.GET("/api/v1/compositions", {
        params: { query: { cursor, prefix: filter.prefix || undefined, artist_id: filter.artist_id || undefined } },
      }),
    ),
  );
  return (
    <ListShell
      kind="compositions"
      list={list}
      prefix={filter.prefix}
      onFilter={(fd) => setFilter((f) => ({ ...f, prefix: str(fd, "prefix") }))}
      renderItem={(c) => (
        <>
          <Link href={`/compositions/${c.id}`}>
            <Name display={c.display} />
          </Link>
          {c.year != null && <span className="muted small"> {c.year}</span>}
        </>
      )}
    />
  );
}

const loadComposition = (id: string) => call(api.GET("/api/v1/compositions/{id}", { params: { path: { id } } }));

export function CompositionDetail({ id }: { id: string }) {
  const t = useTranslations("catalogue");
  const tf = useTranslations("fields");
  const load = useLoad(id, () => loadComposition(id));
  return (
    <Loaded load={load}>
      {(c) => (
        <article>
          <h1>
            <Name display={c.display} />
          </h1>
          <dl className="details">
            {c.iswc && (
              <>
                <dt>{tf("iswc")}</dt>
                <dd dir="ltr">{c.iswc}</dd>
              </>
            )}
            {c.year != null && (
              <>
                <dt>{tf("year")}</dt>
                <dd>{c.year}</dd>
              </>
            )}
          </dl>
          <p className="actions">
            <Link className="button" href={`/compositions/${id}/edit`}>
              {t("edit")}
            </Link>
            <NamesLink entityType="composition" entityId={id} />
            <Link className="button" href={`/recordings/new?composition_id=${id}`}>
              {t("addRecording")}
            </Link>
          </p>
          <h2>{t("credits")}</h2>
          <CreditList credits={c.credits} />
          <h2>{t("recordingsOf", { count: c.recording_count })}</h2>
          {c.recordings.length === 0 ? (
            <p className="muted">{t("recordings.empty")}</p>
          ) : (
            <ul className="entity-list">
              {c.recordings.map((r) => (
                <li key={r.id}>
                  <Link href={`/recordings/${r.id}`}>
                    <Name display={r.display} />
                  </Link>
                </li>
              ))}
            </ul>
          )}
          {c.recording_count > c.recordings.length && (
            <p>
              <Link href={`/recordings?composition_id=${id}`}>{t("allRecordings")}</Link>
            </p>
          )}
        </article>
      )}
    </Loaded>
  );
}

type Ref = { id: string; label: string; choice?: Role };

export function CompositionForm({ initial, onReload }: { initial?: S["CompositionDetail"]; onReload?: () => void }) {
  const t = useTranslations();
  const router = useRouter();
  const [credits, setCredits] = useState<Ref[]>(initial?.credits.map((c) => ({ id: c.artist_id, label: c.artist_name, choice: c.role })) ?? []);
  const s = useSubmit(async (v: Record<string, string>) => {
    const body = {
      title: v.title,
      iswc: text(v.iswc),
      year: int(v.year),
      credits: credits.map((c) => ({ artist_id: c.id, role: c.choice as Role })),
    };
    const saved = initial
      ? await call(api.PATCH("/api/v1/compositions/{id}", { params: { path: { id: initial.id } }, body: { ...body, expected_version: initial.version } }))
      : await call(api.POST("/api/v1/compositions", { body }));
    router.push(`/compositions/${saved.id}`);
  });
  return (
    <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))}>
      {s.error !== undefined && <SaveProblem error={s.error} kind="compositions" onReload={onReload} />}
      <Field name="title" dir="auto" verbatim required defaultValue={initial?.title} hint={t("hints.title")} fieldErrors={s.fieldErrors} />
      <Field name="iswc" dir="ltr" defaultValue={initial?.iswc} hint={t("hints.iswc")} fieldErrors={s.fieldErrors} />
      <Field name="year" type="number" defaultValue={initial?.year} fieldErrors={s.fieldErrors} />
      <RefListEditor
        name="credits"
        legend={t("catalogue.credits")}
        items={credits}
        onChange={setCredits}
        choices={CREDIT_ROLES.compositions}
        choiceLabels="catalogue.roles"
        pickerLabel={t("catalogue.findArtist")}
        search={searchArtists}
        fieldErrors={s.fieldErrors}
      />
      <FormActions submitting={s.submitting} cancelHref={initial ? `/compositions/${initial.id}` : "/compositions"} />
    </form>
  );
}

export function CompositionEdit({ id }: { id: string }) {
  const load = useLoad(id, () => loadComposition(id));
  return <Loaded load={load}>{(c) => <CompositionForm key={c.version} initial={c} onReload={load.reload} />}</Loaded>;
}
