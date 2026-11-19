"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import { Link, useRouter } from "@/i18n/navigation";
import { Name } from "../localization/localized";
import { NamesLink } from "../localization/names";
import { Field, SelectField, formValues, useSubmit } from "../form";
import { ARTIST_KINDS, FormActions, KindFilter, ListShell, Loaded, groupByCategory, SaveProblem, Verbatim, str, text, useLoad, usePaged, type S } from "./common";

type Query = Record<string, string>;

export function ArtistList({ query }: { query: Query }) {
  const t = useTranslations("catalogue");
  const [filter, setFilter] = useState({ prefix: query.prefix ?? "", kind: query.kind ?? "" });
  const list = usePaged<S["Artist"]>(JSON.stringify(filter), (cursor) =>
    call(
      api.GET("/api/v1/artists", {
        params: { query: { cursor, prefix: filter.prefix || undefined, kind: (filter.kind || undefined) as S["ArtistKind"] | undefined } },
      }),
    ),
  );
  return (
    <ListShell
      kind="artists"
      list={list}
      prefix={filter.prefix}
      onFilter={(fd) => setFilter({ prefix: str(fd, "prefix"), kind: str(fd, "kind") })}
      renderItem={(a) => (
        <>
          <Link href={`/artists/${a.id}`}>
            <Name display={a.display} />
          </Link>
          {a.disambiguation && (
            <span className="muted">
              {" "}
              <Verbatim>{a.disambiguation}</Verbatim>
            </span>
          )}
          <span className="muted small"> {t(`artistKinds.${a.kind}`)}</span>
        </>
      )}
    >
      <KindFilter value={filter.kind} options={ARTIST_KINDS} labels="catalogue.artistKinds" />
    </ListShell>
  );
}

const loadArtist = (id: string) => call(api.GET("/api/v1/artists/{id}", { params: { path: { id } } }));

export function ArtistDetail({ id }: { id: string }) {
  const t = useTranslations("catalogue");
  const tf = useTranslations("fields");
  const load = useLoad(id, () => loadArtist(id));
  return (
    <Loaded load={load}>
      {(a) => (
        <article>
          <h1>
            <Name display={a.display} />
          </h1>
          {a.disambiguation && (
            <p className="muted">
              <Verbatim>{a.disambiguation}</Verbatim>
            </p>
          )}
          <dl className="details">
            <dt>{tf("kind")}</dt>
            <dd>{t(`artistKinds.${a.kind}`)}</dd>
            {a.isni && (
              <>
                <dt>{tf("isni")}</dt>
                <dd dir="ltr">{a.isni}</dd>
              </>
            )}
          </dl>
          <p className="actions">
            <Link className="button" href={`/artists/${id}/edit`}>
              {t("edit")}
            </Link>
            <NamesLink entityType="artist" entityId={id} />
          </p>
          <h2>{t("creditedOn")}</h2>
          {a.credits.length === 0 ? (
            <p className="muted">{t("noCredits")}</p>
          ) : (
            groupByCategory(a.credits).map(([category, group]) => (
              <section key={category} data-category={category} aria-label={t(`roleCategories.${category}`)}>
                <h3>{t(`roleCategories.${category}`)}</h3>
                <ul className="credits">
                  {group.map((c) => (
                    <li key={`${c.target_id}-${c.role}`}>
                      <Link href={`/${c.target_type}s/${c.target_id}`}>
                        <Name display={c.title_display} />
                      </Link>
                      <span className="muted">
                        {" "}
                        <Name display={c.role_display} />
                      </span>
                    </li>
                  ))}
                </ul>
              </section>
            ))
          )}
        </article>
      )}
    </Loaded>
  );
}

export function ArtistForm({ initial, onReload }: { initial?: S["ArtistDetail"]; onReload?: () => void }) {
  const t = useTranslations("hints");
  const router = useRouter();
  const s = useSubmit(async (v: Record<string, string>) => {
    const body = { name: v.name, kind: v.kind as S["ArtistKind"], disambiguation: text(v.disambiguation), isni: text(v.isni) };
    const saved = initial
      ? await call(api.PATCH("/api/v1/artists/{id}", { params: { path: { id: initial.id } }, body: { ...body, expected_version: initial.version } }))
      : await call(api.POST("/api/v1/artists", { body }));
    router.push(`/artists/${saved.id}`);
  });
  return (
    <form noValidate onSubmit={(e) => (e.preventDefault(), s.submit(formValues(e.currentTarget)))}>
      {s.error !== undefined && <SaveProblem error={s.error} kind="artists" onReload={onReload} />}
      <Field name="name" dir="auto" verbatim required defaultValue={initial?.name} hint={t("name")} fieldErrors={s.fieldErrors} />
      <SelectField name="kind" options={ARTIST_KINDS} labels="catalogue.artistKinds" defaultValue={initial?.kind} fieldErrors={s.fieldErrors} />
      <Field name="disambiguation" dir="auto" verbatim defaultValue={initial?.disambiguation} hint={t("disambiguation")} fieldErrors={s.fieldErrors} />
      <Field name="isni" dir="ltr" defaultValue={initial?.isni} hint={t("isni")} fieldErrors={s.fieldErrors} />
      <FormActions submitting={s.submitting} cancelHref={initial ? `/artists/${initial.id}` : "/artists"} />
    </form>
  );
}

export function ArtistEdit({ id }: { id: string }) {
  const load = useLoad(id, () => loadArtist(id));
  return <Loaded load={load}>{(a) => <ArtistForm key={a.version} initial={a} onReload={load.reload} />}</Loaded>;
}
