import { notFound } from "next/navigation";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { Link } from "@/i18n/navigation";

const DOCS = ["terms", "privacy", "content-policy"] as const;

export function generateStaticParams() {
  return DOCS.map((doc) => ({ doc }));
}

export const dynamicParams = false;

export default async function Page({ params }: { params: Promise<{ locale: string; doc: string }> }) {
  const { locale, doc } = await params;
  setRequestLocale(locale);
  if (!(DOCS as readonly string[]).includes(doc)) notFound();
  const t = await getTranslations("legal");
  const key = `docs.${doc}` as "docs.terms";
  const sections = t.raw(`${key}.sections`) as { h: string; p: string }[];
  return (
    <article>
      <h1>{t(`${key}.title`)}</h1>
      <p className="muted">{t("draftNotice")}</p>
      {sections.map((s) => (
        <section key={s.h}>
          <h2>{s.h}</h2>
          <p>{s.p}</p>
        </section>
      ))}
      <p>
        <Link href="/legal/notice">{t("noticeTitle")}</Link>
      </p>
    </article>
  );
}
