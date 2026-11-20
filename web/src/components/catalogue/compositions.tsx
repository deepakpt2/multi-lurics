"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import { Link, useRouter } from "@/i18n/navigation";
import { Name } from "../localization/localized";
import { NamesLink } from "../localization/names";
import { Field, SelectField, formValues, useSubmit } from "../form";
import { CREDIT_ROLES, CreditList, FormActions, ListShell, Loaded, RefListEditor, SaveProblem, int, str, text, useLoad, usePaged, type S } from "./common";

type Query = Record<string, string>;
type Role = S["CreditRole"];

const CATEGORIES = ["mainstream", "devotional"] as const;
const RELIGIONS = ["hindu", "sikh", "buddhist", "jain", "christian", "muslim", "other"] as const;

export function CompositionList({ query }: { query: Query }) {
  const t = useTranslations();
  const [filter, setFilter] = useState({
    prefix: query.prefix ?? "",
    artist_id: query.artist_id ?? "",
    category: query.category ?? "",
    raga: query.raga ?? "",
    deity: query.deity ?? "",
    tag: query.tag ?? "",
  });
  const list = usePaged<S["Composition"]>(JSON.stringify(filter), (cursor) =>
    call(
      api.GET("/api/v1/compositions", {
        params: {
          query: {
            cursor,
            prefix: filter.prefix || undefined,
            artist_id: filter.artist_id || undefined,
            category: filter.category || undefined,
            raga: filter.raga || undefined,
            deity: filter.deity || undefined,
            tag: filter.tag || undefined,
          },
        },
      }),
    ),
  );
  return (
    <>
      <form
        className="filters"
        aria-label={t("catalogue.songFilters")}
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
          const fd = new FormData(e.currentTarget);
          setFilter((f) => ({ ...f, category: str(fd, "category"), raga: str(fd, "raga").trim(), deity: str(fd, "deity").trim(), tag: str(fd, "tag").trim() }));
        }}
      >
        <div className="field">
          <label htmlFor="f-category">{t("fields.category")}</label>
          <select id="f-category" name="category" defaultValue={filter.category}>
            <option value="">{t("catalogue.anyType")}</option>
            {CATEGORIES.map((c) => (
              <option key={c} value={c}>
                {t(`catalogue.songCategories.${c}`)}
              </option>
            ))}
          </select>
        </div>
        {(["raga", "deity", "tag"] as const).map((k) => (
          <div className="field" key={k}>
            <label htmlFor={`f-${k}`}>{t(`fields.${k === "tag" ? "tags" : k}`)}</label>
            <input id={`f-${k}`} name={k} dir="auto" defaultValue={filter[k]} />
          </div>
        ))}
        <button type="submit" className="button">
          {t("catalogue.applyFilters")}
        </button>
      </form>
      <CompositionResults list={list} filter={filter} setFilter={setFilter} />
    </>
  );
}

function CompositionResults({
  list,
  filter,
  setFilter,
}: {
  list: ReturnType<typeof usePaged<S["Composition"]>>;
  filter: { prefix: string };
  setFilter: React.Dispatch<React.SetStateAction<{ prefix: string; artist_id: string; category: string; raga: string; deity: string; tag: string }>>;
}) {
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
            <dt>{tf("category")}</dt>
            <dd>{t(`songCategories.${c.category as (typeof CATEGORIES)[number]}`)}</dd>
            {c.raga && (
              <>
                <dt>{tf("raga")}</dt>
                <dd dir="auto">{c.raga}</dd>
              </>
            )}
            {c.religion && (
              <>
                <dt>{tf("religion")}</dt>
                <dd>{t(`religions.${c.religion as (typeof RELIGIONS)[number]}`)}</dd>
              </>
            )}
            {c.deity && (
              <>
                <dt>{tf("deity")}</dt>
                <dd dir="auto">{c.deity}</dd>
              </>
            )}
            {c.tags.length > 0 && (
              <>
                <dt>{tf("tags")}</dt>
                <dd dir="auto">{c.tags.join(", ")}</dd>
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
  const [category, setCategory] = useState<string>(initial?.category ?? "mainstream");
  const s = useSubmit(async (v: Record<string, string>) => {
    const devotional = category === "devotional";
    const body = {
      title: v.title,
      iswc: text(v.iswc),
      year: int(v.year),
      category,
      raga: text(v.raga),
      religion: devotional ? text(v.religion) : null,
      deity: devotional ? text(v.deity) : null,
      tags: (v.tags ?? "").split(/[,،]/).map((x) => x.trim()).filter(Boolean),
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
      <SelectField name="category" options={CATEGORIES} labels="catalogue.songCategories" defaultValue={category} onChange={setCategory} fieldErrors={s.fieldErrors} />
      <Field name="raga" dir="auto" verbatim defaultValue={initial?.raga} hint={t("hints.raga")} fieldErrors={s.fieldErrors} />
      {category === "devotional" && (
        <>
          <SelectField name="religion" options={RELIGIONS} labels="catalogue.religions" optional defaultValue={initial?.religion ?? ""} fieldErrors={s.fieldErrors} />
          <Field name="deity" dir="auto" verbatim defaultValue={initial?.deity} hint={t("hints.deity")} fieldErrors={s.fieldErrors} />
        </>
      )}
      <Field name="tags" dir="auto" verbatim defaultValue={initial?.tags.join(", ")} hint={t("hints.tags")} fieldErrors={s.fieldErrors} />
      <RefListEditor
        name="credits"
        legend={t("catalogue.credits")}
        items={credits}
        onChange={setCredits}
        choices={CREDIT_ROLES.compositions}
        choiceLabels="catalogue.roles"
        pickerLabel={t("catalogue.findArtist")}
        entity="artist"
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
