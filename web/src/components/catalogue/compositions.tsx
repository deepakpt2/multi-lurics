"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import { Link, useRouter } from "@/i18n/navigation";
import { Name } from "../localization/localized";
import { NamesLink } from "../localization/names";
import { MatchHint } from "./match-hint";
import { Field, SelectField, formValues, useSubmit } from "../form";
import { youtubeId } from "@/lib/youtube";
import { CREDIT_ROLES, CreditList, FormActions, ListShell, Loaded, RefListEditor, SaveProblem, int, str, text, useLoad, usePaged, type S } from "./common";

type Query = Record<string, string>;
type Role = S["CreditRole"];

const CATEGORIES = ["mainstream", "devotional"] as const;
const RELIGIONS = ["hindu", "sikh", "buddhist", "jain", "christian", "muslim", "other"] as const;
/** Kinds of devotional piece; the same list is validated by the API and the database. */
export const DEVOTIONAL_FORMS = [
  "bhajan", "kirtan", "stotra", "stuti", "aarti", "mantra", "shloka", "chalisa", "ashtakam", "sahasranamam",
  "suprabhatam", "keerthanam", "namavali", "abhang", "shabad", "qawwali", "naat", "hamd", "marsiya", "hymn",
  "psalm", "carol", "gospel", "sutta", "other",
] as const;

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

/**
 * The song form. With `fillOnly` (anyone signed in, "Add missing details") only the fields that are still
 * empty are shown and sent; what is already filled in stays as it is, because changing it is for the song's
 * creator and moderators. The server enforces the same rule.
 */
export function CompositionForm({ initial, onReload, fillOnly }: { initial?: S["CompositionDetail"]; onReload?: () => void; fillOnly?: boolean }) {
  const t = useTranslations();
  const router = useRouter();
  const [credits, setCredits] = useState<Ref[]>(initial?.credits.map((c) => ({ id: c.artist_id, label: c.artist_name, choice: c.role })) ?? []);
  const [category, setCategory] = useState<string>(initial?.category ?? "mainstream");
  const [typed, setTyped] = useState({ raga: initial?.raga ?? "", deity: initial?.deity ?? "", tag: "" });
  const [video, setVideo] = useState(initial?.youtube_id ?? "");
  const fill = !!fillOnly && !!initial;
  const filled = {
    iswc: !!initial?.iswc,
    year: initial?.year != null,
    raga: !!initial?.raga,
    religion: !!initial?.religion,
    deity: !!initial?.deity,
    form: !!initial?.devotional_form,
    source: !!initial?.source_work,
    occasions: (initial?.occasions.length ?? 0) > 0,
    tags: (initial?.tags.length ?? 0) > 0,
    youtube: !!initial?.youtube_id,
    credits: (initial?.credits.length ?? 0) > 0,
  };
  const show = (k: keyof typeof filled) => !fill || !filled[k];
  const devotional = category === "devotional";
  const anythingToShow = !fill || Object.entries(filled).some(([k, v]) => !v && (devotional || !["religion", "deity", "form", "source", "occasions"].includes(k)));
  const s = useSubmit(async (v: Record<string, string>) => {
    // A field that is not shown is not sent, so it cannot change.
    const when = <T,>(k: keyof typeof filled, value: T) => (show(k) ? value : undefined);
    const list = (x: string | undefined) => (x ?? "").split(/[,،]/).map((y) => y.trim()).filter(Boolean);
    const body = {
      title: fill ? undefined : v.title,
      category: fill ? undefined : category,
      iswc: when("iswc", text(v.iswc)),
      year: when("year", int(v.year)),
      raga: when("raga", text(v.raga)),
      religion: devotional ? when("religion", text(v.religion)) : fill ? undefined : null,
      deity: devotional ? when("deity", text(v.deity)) : fill ? undefined : null,
      devotional_form: devotional ? when("form", text(v.devotional_form)) : fill ? undefined : null,
      source_work: devotional ? when("source", text(v.source_work)) : fill ? undefined : null,
      occasions: devotional ? when("occasions", list(v.occasions)) : fill ? undefined : [],
      youtube: when("youtube", text(v.youtube)),
      tags: when("tags", list(v.tags)),
      credits: when("credits", credits.map((c) => ({ artist_id: c.id, role: c.choice as Role }))),
    };
    const saved = initial
      ? await call(api.PATCH("/api/v1/compositions/{id}", { params: { path: { id: initial.id } }, body: { ...body, expected_version: initial.version } }))
      : await call(api.POST("/api/v1/compositions", { body: { ...body, title: v.title } }));
    if (fill) router.back();
    else router.push(`/compositions/${saved.id}`);
  });
  return (
    <form
      noValidate
      onInput={(e) => {
        const el = e.target as unknown as HTMLInputElement;
        if (el.name === "raga" || el.name === "deity") setTyped((x) => ({ ...x, [el.name]: el.value }));
        if (el.name === "youtube") setVideo(el.value);
        if (el.name === "tags") setTyped((x) => ({ ...x, tag: el.value.split(/[,،]/).pop() ?? "" }));
      }}
      onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))}
    >
      {s.error !== undefined && <SaveProblem error={s.error} kind="compositions" onReload={onReload} />}
      {fill && <p className="muted">{anythingToShow ? t("catalogue.fillHint") : t("catalogue.nothingMissing")}</p>}
      {!fill && <Field name="title" dir="auto" verbatim required defaultValue={initial?.title} hint={t("hints.title")} fieldErrors={s.fieldErrors} />}
      {show("iswc") && <Field name="iswc" dir="ltr" defaultValue={initial?.iswc} hint={t("hints.iswc")} fieldErrors={s.fieldErrors} />}
      {show("year") && <Field name="year" type="number" defaultValue={initial?.year} fieldErrors={s.fieldErrors} />}
      {!fill && <SelectField name="category" options={CATEGORIES} labels="catalogue.songCategories" defaultValue={category} onChange={setCategory} fieldErrors={s.fieldErrors} />}
      {show("raga") && (
        <>
          <Field name="raga" dir="auto" verbatim defaultValue={initial?.raga} hint={t("hints.raga")} fieldErrors={s.fieldErrors} />
          <MatchHint type="raga" value={typed.raga} />
        </>
      )}
      {devotional && (
        <>
          {show("religion") && <SelectField name="religion" options={RELIGIONS} labels="catalogue.religions" optional defaultValue={initial?.religion ?? ""} fieldErrors={s.fieldErrors} />}
          {show("deity") && (
            <>
              <Field name="deity" dir="auto" verbatim defaultValue={initial?.deity} hint={t("hints.deity")} fieldErrors={s.fieldErrors} />
              <MatchHint type="deity" value={typed.deity} />
            </>
          )}
          {show("form") && <SelectField name="devotional_form" options={DEVOTIONAL_FORMS} labels="catalogue.devotionalForms" optional defaultValue={initial?.devotional_form ?? ""} fieldErrors={s.fieldErrors} />}
          {show("source") && <Field name="source_work" dir="auto" verbatim defaultValue={initial?.source_work} hint={t("hints.sourceWork")} fieldErrors={s.fieldErrors} />}
          {show("occasions") && <Field name="occasions" dir="auto" verbatim defaultValue={initial?.occasions.join(", ")} hint={t("hints.occasions")} fieldErrors={s.fieldErrors} />}
        </>
      )}
      {show("youtube") && (
        <>
          <Field name="youtube" dir="ltr" defaultValue={initial?.youtube_id ? `https://youtu.be/${initial.youtube_id}` : ""} hint={t("hints.youtube")} fieldErrors={s.fieldErrors} />
          {video.trim() !== "" && youtubeId(video) === null && (
            <p className="muted small" role="status">
              {t("hints.youtubeUnrecognised")}
            </p>
          )}
        </>
      )}
      {show("tags") && (
        <>
          <Field name="tags" dir="auto" verbatim defaultValue={initial?.tags.join(", ")} hint={t("hints.tags")} fieldErrors={s.fieldErrors} />
          <MatchHint type="tag" value={typed.tag} />
        </>
      )}
      {show("credits") && (
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
      )}
      <FormActions submitting={s.submitting} disabled={fill && !anythingToShow} cancelHref={initial ? (fill ? "/" : `/compositions/${initial.id}`) : "/compositions"} />
    </form>
  );
}

export function CompositionEdit({ id, fillOnly }: { id: string; fillOnly?: boolean }) {
  const load = useLoad(id, () => loadComposition(id));
  return <Loaded load={load}>{(c) => <CompositionForm key={c.version} initial={c} onReload={load.reload} fillOnly={fillOnly} />}</Loaded>;
}
