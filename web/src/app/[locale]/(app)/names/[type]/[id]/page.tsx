import { notFound } from "next/navigation";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { NamesView, type EntityType } from "@/components/localization/names";

const TYPES: readonly string[] = ["composition", "recording", "artist", "album", "label", "publisher", "genre", "raga", "deity", "tag", "credit_role"];

export default async function Page({ params }: { params: Promise<{ locale: string; type: string; id: string }> }) {
  const { locale, type, id } = await params;
  setRequestLocale(locale);
  if (!TYPES.includes(type)) notFound();
  const t = await getTranslations("localization");
  return (
    <>
      <h1>{t("title")}</h1>
      <NamesView entityType={type as EntityType} entityId={id} />
    </>
  );
}
