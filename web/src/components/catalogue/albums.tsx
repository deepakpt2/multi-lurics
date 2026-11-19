"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import { Link, useRouter } from "@/i18n/navigation";
import { Name } from "../localization/localized";
import { NamesLink } from "../localization/names";
import { Field, SelectField, formValues, useSubmit } from "../form";
import {
  ALBUM_KINDS,
  CREDIT_ROLES,
  CreditList,
  Duration,
  FormActions,
  KindFilter,
  ListShell,
  Loaded,
  RefListEditor,
  SaveProblem,
  int,
  str,
  text,
  useLoad,
  usePaged,
  type S,
} from "./common";
import { searchArtists, searchRecordings } from "./search";

type Query = Record<string, string>;
type Role = S["CreditRole"];

export function AlbumList({ query }: { query: Query }) {
  const t = useTranslations("catalogue");
  const [filter, setFilter] = useState({ prefix: query.prefix ?? "", kind: query.kind ?? "", artist_id: query.artist_id ?? "" });
  const list = usePaged<S["Album"]>(JSON.stringify(filter), (cursor) =>
    call(
      api.GET("/api/v1/albums", {
        params: {
          query: {
            cursor,
            prefix: filter.prefix || undefined,
            kind: (filter.kind || undefined) as S["AlbumKind"] | undefined,
            artist_id: filter.artist_id || undefined,
          },
        },
      }),
    ),
  );
  return (
    <ListShell
      kind="albums"
      list={list}
      prefix={filter.prefix}
      onFilter={(fd) => setFilter((f) => ({ ...f, prefix: str(fd, "prefix"), kind: str(fd, "kind") }))}
      renderItem={(a) => (
        <>
          <Link href={`/albums/${a.id}`}>
            <Name display={a.display} />
          </Link>
          <span className="muted small">
            {" "}
            {t(`albumKinds.${a.kind}`)}
            {a.release_year != null && ` · ${a.release_year}`}
          </span>
        </>
      )}
    >
      <KindFilter value={filter.kind} options={ALBUM_KINDS} labels="catalogue.albumKinds" />
    </ListShell>
  );
}

const loadAlbum = (id: string) => call(api.GET("/api/v1/albums/{id}", { params: { path: { id } } }));

export function AlbumDetail({ id }: { id: string }) {
  const t = useTranslations("catalogue");
  const tf = useTranslations("fields");
  const load = useLoad(id, () => loadAlbum(id));
  return (
    <Loaded load={load}>
      {(a) => (
        <article>
          <h1>
            <Name display={a.display} />
          </h1>
          <dl className="details">
            <dt>{tf("kind")}</dt>
            <dd>{t(`albumKinds.${a.kind}`)}</dd>
            {a.release_year != null && (
              <>
                <dt>{tf("release_year")}</dt>
                <dd>{a.release_year}</dd>
              </>
            )}
            {a.barcode && (
              <>
                <dt>{tf("barcode")}</dt>
                <dd dir="ltr">{a.barcode}</dd>
              </>
            )}
          </dl>
          <p className="actions">
            <Link className="button" href={`/albums/${id}/edit`}>
              {t("edit")}
            </Link>
            <NamesLink entityType="album" entityId={id} />
          </p>
          <h2>{t("credits")}</h2>
          <CreditList credits={a.credits} />
          <h2>{t("tracks")}</h2>
          {a.tracks.length === 0 ? (
            <p className="muted">{t("noTracks")}</p>
          ) : (
            <ol className="tracks">
              {a.tracks.map((tr) => (
                <li key={tr.position} value={tr.position}>
                  <Link href={`/recordings/${tr.recording_id}`}>
                    <Name display={tr.display} />
                  </Link>
                  <span className="muted small">
                    {" "}
                    <Duration ms={tr.duration_ms} />
                  </span>
                </li>
              ))}
            </ol>
          )}
        </article>
      )}
    </Loaded>
  );
}

type Ref<C> = { id: string; label: string; choice?: C };

export function AlbumForm({ initial, onReload }: { initial?: S["AlbumDetail"]; onReload?: () => void }) {
  const t = useTranslations();
  const router = useRouter();
  const [credits, setCredits] = useState<Ref<Role>[]>(initial?.credits.map((c) => ({ id: c.artist_id, label: c.artist_name, choice: c.role })) ?? []);
  const [tracks, setTracks] = useState<Ref<string>[]>(initial?.tracks.map((tr) => ({ id: tr.recording_id, label: tr.title })) ?? []);
  const s = useSubmit(async (v: Record<string, string>) => {
    const body = {
      title: v.title,
      kind: v.kind as S["AlbumKind"],
      release_year: int(v.release_year),
      barcode: text(v.barcode),
      credits: credits.map((c) => ({ artist_id: c.id, role: c.choice as Role })),
      tracks: tracks.map((tr) => tr.id),
    };
    const saved = initial
      ? await call(api.PATCH("/api/v1/albums/{id}", { params: { path: { id: initial.id } }, body: { ...body, expected_version: initial.version } }))
      : await call(api.POST("/api/v1/albums", { body }));
    router.push(`/albums/${saved.id}`);
  });
  return (
    <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))}>
      {s.error !== undefined && <SaveProblem error={s.error} kind="albums" onReload={onReload} />}
      <Field name="title" dir="auto" verbatim required defaultValue={initial?.title} hint={t("hints.title")} fieldErrors={s.fieldErrors} />
      <SelectField name="kind" options={ALBUM_KINDS} labels="catalogue.albumKinds" defaultValue={initial?.kind} fieldErrors={s.fieldErrors} />
      <Field name="release_year" type="number" defaultValue={initial?.release_year} fieldErrors={s.fieldErrors} />
      <Field name="barcode" dir="ltr" defaultValue={initial?.barcode} hint={t("hints.barcode")} fieldErrors={s.fieldErrors} />
      <RefListEditor
        name="credits"
        legend={t("catalogue.credits")}
        items={credits}
        onChange={setCredits}
        choices={CREDIT_ROLES.albums}
        choiceLabels="catalogue.roles"
        pickerLabel={t("catalogue.findArtist")}
        search={searchArtists}
        fieldErrors={s.fieldErrors}
      />
      <RefListEditor
        name="tracks"
        legend={t("catalogue.tracks")}
        items={tracks}
        onChange={setTracks}
        pickerLabel={t("catalogue.findRecording")}
        search={searchRecordings}
        fieldErrors={s.fieldErrors}
        ordered
      />
      <FormActions submitting={s.submitting} cancelHref={initial ? `/albums/${initial.id}` : "/albums"} />
    </form>
  );
}

export function AlbumEdit({ id }: { id: string }) {
  const load = useLoad(id, () => loadAlbum(id));
  return <Loaded load={load}>{(a) => <AlbumForm key={a.version} initial={a} onReload={load.reload} />}</Loaded>;
}
