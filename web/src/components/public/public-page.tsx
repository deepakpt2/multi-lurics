import type { Metadata } from "next";
import { notFound, permanentRedirect } from "next/navigation";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { Link } from "@/i18n/navigation";
import { routing } from "@/i18n/routing";
import { decodeSlug, describe, fetchPage, jsonLd, pagePath, pageUrl, safeJson, siteUrl, type PublicPage, type Scope } from "@/lib/seo";
import { Name } from "../localization/localized";
import { ContributeButton } from "./contribute";
import { LyricPanes } from "./lyric-panes";
import { parseIds } from "@/lib/lyric-panes";
import { PageStats } from "./page-stats";
import { VideoEmbed } from "./video-embed";

type Params = Promise<{ locale: string; slug: string }>;

/** Resolves the address, sending every non-canonical spelling to the canonical one (permanent). */
async function load(scope: Scope, params: Params, suffix = "") {
  const { locale, slug: raw } = await params;
  const slug = decodeSlug(raw);
  const r = await fetchPage(scope, locale, slug);
  if (r.kind === "missing") notFound();
  // A view of the page (such as its full-screen lyrics) keeps its tail when the address moves.
  if (r.kind === "moved") permanentRedirect(`${pagePath(r.to, scope)}${suffix}`);
  return { locale, page: r.page };
}

export function publicMetadata(scope: Scope) {
  return async function generateMetadata({ params }: { params: Params }): Promise<Metadata> {
    const { locale, slug: raw } = await params;
    const r = await fetchPage(scope, locale, decodeSlug(raw));
    if (r.kind !== "page") return { robots: { index: false, follow: false } };
    const p = r.page;
    const t = await getTranslations({ locale, namespace: "public" });
    const artist = p.credits.find((c) => ["performer", "primary_artist", "composer"].includes(c.role))?.artist.label.text;
    const title = t(`title.${scope}`, { name: p.title.text, artist: artist ?? "" }).replace(/\s+/g, " ").trim();
    const description = describe(p) ?? t(`description.${scope}`, { name: p.title.text });
    const url = pageUrl(p.canonical, scope);
    const languages = Object.fromEntries(p.alternates.map((a) => [a.locale, pageUrl(a, scope)]));
    return {
      metadataBase: new URL(siteUrl()),
      title,
      description,
      // One address per page: query strings and slug variants all point here.
      alternates: { canonical: url, languages: { ...languages, "x-default": pageUrl(p.x_default, scope) } },
      openGraph: {
        type: scope === "artist" ? "profile" : "music.song",
        title,
        description,
        url,
        siteName: (await getTranslations({ locale, namespace: "app" }))("name"),
        locale: locale,
        alternateLocale: p.alternates.map((a) => a.locale).filter((l) => l !== locale),
      },
      robots: { index: true, follow: true },
    };
  };
}

function PageLinkItem({ link }: { link: PublicPage["recordings"][number] }) {
  return link.page ? (
    <Link href={`/${link.scope}/${link.page.slug}`} locale={link.page.locale as never}>
      <Name display={link.label} />
    </Link>
  ) : (
    <Name display={link.label} />
  );
}

export function publicPage(scope: Scope) {
  return async function Page({ params }: { params: Params }) {
    const { locale, page: p } = await load(scope, params);
    setRequestLocale(locale);
    const t = await getTranslations("public");
    const tr = await getTranslations("catalogue");
    const tf = await getTranslations("fields");
    const others = p.alternates.filter((a) => a.locale !== p.locale);
    const lead = p.lyrics[0];
    const recordingId = p.scope === "recording" ? p.id : lead?.recording_id;
    const sourceId = p.lyrics.find((l) => l.type === "original" && l.recording_id === recordingId)?.id;
    const songPath = `/${scope}/${encodeURIComponent(p.canonical.slug)}`;
    return (
      <article>
        <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: safeJson(jsonLd(p)) }} />
        <h1>
          <Name display={p.title} />
        </h1>
        <p className="muted">
          {[p.parent ? t("recordingOf") : undefined, p.year, p.detail].filter((x) => x != null && x !== "").join(" · ")}
          {p.parent && (
            <>
              {" "}
              <PageLinkItem link={p.parent} />
            </>
          )}
        </p>
        {(p.scope === "song" || p.scope === "artist") && <PageStats kind={p.scope} id={p.id} lyricIds={p.lyrics.map((l) => l.id)} />}
        <p className="page-actions">
          <ContributeButton target={p.scope === "artist" ? { kind: "artist", id: p.id } : { kind: p.scope === "song" ? "song" : "recording", id: p.id, recordingId, sourceId }} />
        </p>
        {p.scope === "song" && (
          <dl className="facts">
            <dt>{tf("category")}</dt>
            <dd>{tr(`songCategories.${(p.category ?? "mainstream") as "mainstream"}`)}</dd>
            {p.raga && (
              <>
                <dt>{tf("raga")}</dt>
                <dd dir="auto">{p.raga}</dd>
              </>
            )}
            {p.religion && (
              <>
                <dt>{tf("religion")}</dt>
                <dd>{tr(`religions.${p.religion as "hindu"}`)}</dd>
              </>
            )}
            {p.deity && (
              <>
                <dt>{tf("deity")}</dt>
                <dd dir="auto">{p.deity}</dd>
              </>
            )}
            {p.devotional_form && (
              <>
                <dt>{tf("devotional_form")}</dt>
                <dd>{tr(`devotionalForms.${p.devotional_form as "bhajan"}`)}</dd>
              </>
            )}
            {p.source_work && (
              <>
                <dt>{tf("source_work")}</dt>
                <dd dir="auto">{p.source_work}</dd>
              </>
            )}
            {p.occasions.length > 0 && (
              <>
                <dt>{tf("occasions")}</dt>
                <dd dir="auto">{p.occasions.join(", ")}</dd>
              </>
            )}
            {p.tags.length > 0 && (
              <>
                <dt>{tf("tags")}</dt>
                <dd dir="auto">{p.tags.join(", ")}</dd>
              </>
            )}
          </dl>
        )}
        {p.credits.length > 0 && (
          <>
            <h2>{t("credits")}</h2>
            <ul className="entity-list">
              {p.credits.map((c) => (
                <li key={`${c.artist.id}-${c.role}`}>
                  <PageLinkItem link={c.artist} /> <span className="muted small">{tr(`roles.${c.role as "composer"}`)}</span>
                </li>
              ))}
            </ul>
          </>
        )}
        {p.songs.length > 0 && (
          <>
            <h2>{t("songs")}</h2>
            <ul className="entity-list">
              {p.songs.map((s) => (
                <li key={s.id}>
                  <PageLinkItem link={s} />
                </li>
              ))}
            </ul>
          </>
        )}
        {p.recordings.length > 0 && (
          <>
            <h2>{t("recordings")}</h2>
            <ul className="entity-list">
              {p.recordings.map((r) => (
                <li key={r.id}>
                  <PageLinkItem link={r} />
                </li>
              ))}
            </ul>
          </>
        )}
        {p.youtube_id && <VideoEmbed id={p.youtube_id} title={p.title.text} />}
        {p.lyrics.length > 0 && recordingId && (
          <section aria-labelledby="lyrics-title">
            <h2 id="lyrics-title">{t("lyrics")}</h2>
            <LyricPanes lyrics={p.lyrics} songPath={songPath} pageLocale={p.locale} recordingId={recordingId} sourceId={sourceId} />
          </section>
        )}
        {p.related.length > 0 && (
          <section aria-labelledby="related-title">
            <h2 id="related-title">{t("related.title")}</h2>
            <ul className="related-songs">
              {p.related.map((r) => (
                <li key={r.song.id}>
                  <PageLinkItem link={r.song} />
                  <span className="muted small">
                    {" "}
                    {r.reasons.map((x) => t(`related.reasons.${x as "album"}`)).join(" · ")}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        )}
        {others.length > 0 && (
          <nav aria-label={t("otherLanguages")}>
            <h2>{t("otherLanguages")}</h2>
            <ul className="entity-list">
              {others.map((a) => (
                <li key={a.locale}>
                  <Link href={`/${scope}/${a.slug}`} locale={a.locale as never} lang={a.locale} hrefLang={a.locale}>
                    {t(`localeNames.${a.locale as "en"}`)}
                  </Link>
                </li>
              ))}
            </ul>
          </nav>
        )}
      </article>
    );
  };
}

/**
 * A song's lyrics on a page of their own: wide, with the same columns, reading mode and "+" as the song
 * page. `?l=id,id` names the columns that are open, so a full-screen view can be shared as it stands.
 */
export function publicLyricsPage(scope: "song" | "recording") {
  return async function Page({ params, searchParams }: { params: Params; searchParams: Promise<Record<string, string | string[] | undefined>> }) {
    const sp = await searchParams;
    const l = parseIds(sp.l).join(",");
    const { locale, page: p } = await load(scope, params, `/lyrics${l ? `?l=${encodeURIComponent(l)}` : ""}`);
    setRequestLocale(locale);
    const t = await getTranslations("public");
    const lead = p.lyrics[0];
    const recordingId = scope === "recording" ? p.id : lead?.recording_id;
    if (!lead || !recordingId) notFound();
    const sourceId = p.lyrics.find((l) => l.type === "original" && l.recording_id === recordingId)?.id;
    const songPath = `/${scope}/${encodeURIComponent(p.canonical.slug)}`;
    return (
      <article className="full-lyrics">
        <p>
          <Link href={songPath} locale={p.locale as never}>
            {t("panes.backToSong")}
          </Link>
        </p>
        <h1>
          <Name display={p.title} />
        </h1>
        <LyricPanes
          lyrics={p.lyrics}
          songPath={songPath}
          pageLocale={p.locale}
          recordingId={recordingId}
          sourceId={sourceId}
          initial={parseIds(sp.l)}
          fullScreen
        />
      </article>
    );
  };
}

export function publicLyricsMetadata(scope: "song" | "recording") {
  return async function generateMetadata({ params }: { params: Params }): Promise<Metadata> {
    const { locale, slug: raw } = await params;
    const r = await fetchPage(scope, locale, decodeSlug(raw));
    const t = await getTranslations({ locale, namespace: "public.panes" });
    if (r.kind !== "page") return { robots: { index: false, follow: false } };
    return {
      title: t("pageTitle", { name: r.page.title.text }),
      // The song's own page is the one to index; this is a view of it.
      alternates: { canonical: pageUrl(r.page.canonical, scope) },
      robots: { index: false, follow: true },
    };
  };
}

export const supportedLocales = routing.locales;
