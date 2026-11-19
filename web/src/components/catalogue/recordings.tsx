"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import { Link, useRouter } from "@/i18n/navigation";
import { Name } from "../localization/localized";
import { NamesLink } from "../localization/names";
import { LyricsSection } from "../lyrics/lyrics";
import { Field, formValues, useFieldError, useSubmit } from "../form";
import {
  CREDIT_ROLES,
  CreditList,
  Duration,
  FormActions,
  ListShell,
  Loaded,
  Picker,
  RELATIONSHIP_KINDS,
  RefListEditor,
  SaveProblem,
  Verbatim,
  int,
  str,
  text,
  useLoad,
  usePaged,
  type S,
} from "./common";
import { searchCompositions, searchRecordings } from "./search";

type Query = Record<string, string>;
type Ref<C> = { id: string; label: string; choice?: C };

export function RecordingList({ query }: { query: Query }) {
  const [filter, setFilter] = useState({
    prefix: query.prefix ?? "",
    composition_id: query.composition_id ?? "",
    artist_id: query.artist_id ?? "",
    album_id: query.album_id ?? "",
  });
  const list = usePaged<S["Recording"]>(JSON.stringify(filter), (cursor) =>
    call(
      api.GET("/api/v1/recordings", {
        params: {
          query: {
            cursor,
            prefix: filter.prefix || undefined,
            composition_id: filter.composition_id || undefined,
            artist_id: filter.artist_id || undefined,
            album_id: filter.album_id || undefined,
          },
        },
      }),
    ),
  );
  return (
    <ListShell
      kind="recordings"
      list={list}
      prefix={filter.prefix}
      onFilter={(fd) => setFilter((f) => ({ ...f, prefix: str(fd, "prefix") }))}
      newHref={filter.composition_id ? `/recordings/new?composition_id=${filter.composition_id}` : undefined}
      renderItem={(r) => (
        <>
          <Link href={`/recordings/${r.id}`}>
            <Name display={r.display} />
          </Link>
          <span className="muted small">
            {" "}
            <Duration ms={r.duration_ms} />
          </span>
        </>
      )}
    />
  );
}

const loadRecording = (id: string) => call(api.GET("/api/v1/recordings/{id}", { params: { path: { id } } }));

function Related({ items, phrase }: { items: S["RelatedRecording"][]; phrase: "relationshipOut" | "relationshipIn" }) {
  const t = useTranslations("catalogue");
  return (
    <ul className="entity-list">
      {items.map((r) => (
        <li key={`${r.kind}-${r.recording_id}`}>
          <span className="muted">{t(`${phrase}.${r.kind}`)} </span>
          <Link href={`/recordings/${r.recording_id}`}>
            <Name display={r.display} />
          </Link>
        </li>
      ))}
    </ul>
  );
}

export function RecordingDetail({ id }: { id: string }) {
  const t = useTranslations("catalogue");
  const tf = useTranslations("fields");
  const load = useLoad(id, () => loadRecording(id));
  return (
    <Loaded load={load}>
      {(r) => (
        <article>
          <h1>
            <Name display={r.display} />
          </h1>
          <p>
            <span className="muted">{t("recordingOf")} </span>
            <Link href={`/compositions/${r.composition.id}`}>
              <Name display={r.composition.display} />
            </Link>
          </p>
          <dl className="details">
            {r.isrc && (
              <>
                <dt>{tf("isrc")}</dt>
                <dd dir="ltr">{r.isrc}</dd>
              </>
            )}
            {r.duration_ms != null && (
              <>
                <dt>{tf("duration")}</dt>
                <dd>
                  <Duration ms={r.duration_ms} />
                </dd>
              </>
            )}
            {r.year != null && (
              <>
                <dt>{tf("year")}</dt>
                <dd>{r.year}</dd>
              </>
            )}
          </dl>
          <p className="actions">
            <Link className="button" href={`/recordings/${id}/edit`}>
              {t("edit")}
            </Link>
            <NamesLink entityType="recording" entityId={id} />
            <Link className="button" href={`/recordings/new?composition_id=${r.composition.id}&related=${id}&kind=cover`}>
              {t("addCover")}
            </Link>
          </p>
          <LyricsSection recordingId={id} />
          <h2>{t("credits")}</h2>
          <CreditList credits={r.credits} />
          {(r.relationships.length > 0 || r.related_from.length > 0) && (
            <>
              <h2>{t("relationships")}</h2>
              <Related items={r.relationships} phrase="relationshipOut" />
              <Related items={r.related_from} phrase="relationshipIn" />
            </>
          )}
          {r.albums.length > 0 && (
            <>
              <h2>{t("appearsOn")}</h2>
              <ul className="entity-list">
                {r.albums.map((a) => (
                  <li key={a.album_id}>
                    <Link href={`/albums/${a.album_id}`}>
                      <Name display={a.display} />
                    </Link>
                    <span className="muted small"> {t("trackNumber", { n: a.position })}</span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </article>
      )}
    </Loaded>
  );
}

/** The one composition a recording belongs to; chosen from existing ones only. */
function CompositionChoice({
  value,
  onChange,
  fieldErrors,
}: {
  value: { id: string; label: string } | null;
  onChange: (v: { id: string; label: string } | null) => void;
  fieldErrors: { field: string; code: string }[];
}) {
  const t = useTranslations("catalogue");
  const err = useFieldError(fieldErrors, "composition_id");
  return (
    <fieldset className="ref-list" aria-invalid={err ? true : undefined}>
      <legend>{t("composition")}</legend>
      {err && <p className="field-error">{err}</p>}
      {value ? (
        <p>
          <Verbatim>{value.label}</Verbatim>{" "}
          <button type="button" className="button" onClick={() => onChange(null)}>
            {t("change")}
          </button>
        </p>
      ) : (
        <Picker label={t("findComposition")} search={searchCompositions} onPick={onChange} />
      )}
    </fieldset>
  );
}

type Rel = S["RelationshipKind"];
type Role = S["CreditRole"];

export function RecordingForm({
  initial,
  preset,
  onReload,
}: {
  initial?: S["RecordingDetail"];
  preset?: { composition?: { id: string; label: string }; related?: Ref<Rel> };
  onReload?: () => void;
}) {
  const t = useTranslations();
  const router = useRouter();
  const [composition, setComposition] = useState(
    initial ? { id: initial.composition.id, label: initial.composition.title } : (preset?.composition ?? null),
  );
  const [credits, setCredits] = useState<Ref<Role>[]>(initial?.credits.map((c) => ({ id: c.artist_id, label: c.artist_name, choice: c.role })) ?? []);
  const [rels, setRels] = useState<Ref<Rel>[]>(
    initial?.relationships.map((r) => ({ id: r.recording_id, label: r.title, choice: r.kind })) ?? (preset?.related ? [preset.related] : []),
  );
  const s = useSubmit(async (v: Record<string, string>) => {
    const seconds = int(v.duration);
    const body = {
      composition_id: composition?.id ?? null,
      title: v.title,
      isrc: text(v.isrc),
      duration_ms: seconds == null ? null : Math.round(seconds * 1000),
      year: int(v.year),
      credits: credits.map((c) => ({ artist_id: c.id, role: c.choice as Role })),
      relationships: rels.map((r) => ({ kind: r.choice as Rel, related_recording_id: r.id })),
    };
    const saved = initial
      ? await call(
          api.PATCH("/api/v1/recordings/{id}", {
            params: { path: { id: initial.id } },
            body: { ...body, composition_id: body.composition_id ?? undefined, expected_version: initial.version },
          }),
        )
      : await call(api.POST("/api/v1/recordings", { body }));
    router.push(`/recordings/${saved.id}`);
  });
  // The duration input is in seconds; server errors refer to `duration_ms`.
  const fieldErrors = s.fieldErrors.map((e) => (e.field === "duration_ms" ? { ...e, field: "duration" } : e));
  return (
    <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))}>
      {s.error !== undefined && <SaveProblem error={s.error} kind="recordings" onReload={onReload} />}
      <CompositionChoice value={composition} onChange={setComposition} fieldErrors={fieldErrors} />
      <Field name="title" dir="auto" verbatim required defaultValue={initial?.title} hint={t("hints.title")} fieldErrors={fieldErrors} />
      <Field name="isrc" dir="ltr" defaultValue={initial?.isrc} hint={t("hints.isrc")} fieldErrors={fieldErrors} />
      <Field
        name="duration"
        type="number"
        defaultValue={initial?.duration_ms != null ? Math.round(initial.duration_ms / 1000) : undefined}
        hint={t("hints.duration")}
        fieldErrors={fieldErrors}
      />
      <Field name="year" type="number" defaultValue={initial?.year} fieldErrors={fieldErrors} />
      <RefListEditor
        name="credits"
        legend={t("catalogue.credits")}
        items={credits}
        onChange={setCredits}
        choices={CREDIT_ROLES.recordings}
        choiceLabels="catalogue.roles"
        pickerLabel={t("catalogue.findArtist")}
        entity="artist"
        fieldErrors={fieldErrors}
      />
      <RefListEditor
        name="relationships"
        legend={t("catalogue.relationships")}
        items={rels}
        onChange={setRels}
        choices={RELATIONSHIP_KINDS}
        choiceLabels="catalogue.relationshipKinds"
        pickerLabel={t("catalogue.findRecording")}
        search={searchRecordings}
        fieldErrors={fieldErrors}
      />
      <p className="muted small">{t("hints.relationships")}</p>
      <FormActions submitting={s.submitting} cancelHref={initial ? `/recordings/${initial.id}` : "/recordings"} />
    </form>
  );
}

/** New recording, optionally preset from links like "Add recording" / "Add a cover". */
export function RecordingNew({ query }: { query: Query }) {
  const { composition_id, related, kind } = query;
  const load = useLoad(`${composition_id}|${related}`, async () => {
    const [c, r] = await Promise.all([
      composition_id ? call(api.GET("/api/v1/compositions/{id}", { params: { path: { id: composition_id } } })) : null,
      related ? loadRecording(related) : null,
    ]);
    const relKind = RELATIONSHIP_KINDS.find((k) => k === kind) ?? "cover";
    return {
      composition: c ? { id: c.id, label: c.title } : undefined,
      related: r ? { id: r.id, label: r.title, choice: relKind } : undefined,
    };
  });
  return <Loaded load={load}>{(preset) => <RecordingForm preset={preset} />}</Loaded>;
}

export function RecordingEdit({ id }: { id: string }) {
  const load = useLoad(id, () => loadRecording(id));
  return <Loaded load={load}>{(r) => <RecordingForm key={r.version} initial={r} onReload={load.reload} />}</Loaded>;
}
