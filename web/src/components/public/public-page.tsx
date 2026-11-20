import type { Metadata } from "next";
import { notFound, permanentRedirect } from "next/navigation";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { Link } from "@/i18n/navigation";
import { routing } from "@/i18n/routing";
import { decodeSlug, describe, fetchPage, jsonLd, pagePath, pageUrl, safeJson, siteUrl, type PublicPage, type Scope } from "@/lib/seo";
import { Name } from "../localization/localized";
import { PageStats } from "./page-stats";

type Params = Promise<{ locale: string; slug: string }>;

/** Resolves the address, sending every non-canonical spelling to the canonical one (permanent). */
async function load(scope: Scope, params: Params) {
  const { locale, slug: raw } = await params;
  const slug = decodeSlug(raw);
  const r = await fetchPage(scope, locale, slug);
  if (r.kind === "missing") notFound();
  if (r.kind === "moved") permanentRedirect(pagePath(r.to, scope));
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
    const tl = await getTranslations("lyrics");
    const tf = await getTranslations("fields");
    const others = p.alternates.filter((a) => a.locale !== p.locale);
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
        {p.lyrics.map((l) => (
          <section key={l.id} aria-label={t("lyricsLabel", { type: tl(`types.${l.type as "original"}`) })}>
            <h2>
              {t("lyrics")} <small className="badge">{tl(`types.${l.type as "original"}`)}</small>
            </h2>
            <pre className="lyric-body" lang={l.language} dir={l.dir}>
              {l.body}
            </pre>
          </section>
        ))}
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

export const supportedLocales = routing.locales;
