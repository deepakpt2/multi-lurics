import { getTranslations, setRequestLocale } from "next-intl/server";
import { ContributeSong } from "@/components/contribute/contribute-song";
import { parsePrefill } from "@/lib/contribute";

export default async function Page({ params, searchParams }: { params: Promise<{ locale: string }>; searchParams: Promise<Record<string, string | string[] | undefined>> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("contribute");
  return (
    <>
      <h1>{t("title")}</h1>
      <p className="lead">{t("intro")}</p>
      <ContributeSong prefill={parsePrefill(await searchParams)} />
    </>
  );
}
