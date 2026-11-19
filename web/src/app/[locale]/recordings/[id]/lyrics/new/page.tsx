import { getTranslations, setRequestLocale } from "next-intl/server";
import { ContributionNew } from "@/components/lyrics/lyrics";

type Props = { params: Promise<{ locale: string; id: string }>; searchParams: Promise<Record<string, string | string[] | undefined>> };

export default async function Page({ params, searchParams }: Props) {
  const { locale, id } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("lyrics");
  const query = Object.fromEntries(Object.entries(await searchParams).filter((e): e is [string, string] => typeof e[1] === "string"));
  return (
    <>
      <h1>{t("newTitle")}</h1>
      <ContributionNew recordingId={id} query={query} />
    </>
  );
}
