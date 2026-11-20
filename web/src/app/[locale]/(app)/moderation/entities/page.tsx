import { getTranslations, setRequestLocale } from "next-intl/server";
import { ProposalQueue } from "@/components/entities/proposals";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("entities");
  return (
    <>
      <h1>{t("queueTitle")}</h1>
      <ProposalQueue />
    </>
  );
}
