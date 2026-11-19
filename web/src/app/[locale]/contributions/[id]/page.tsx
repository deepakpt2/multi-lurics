import { setRequestLocale } from "next-intl/server";
import { ContributionDetail } from "@/components/lyrics/lyrics";

export default async function Page({ params }: { params: Promise<{ locale: string; id: string }> }) {
  const { locale, id } = await params;
  setRequestLocale(locale);
  return <ContributionDetail id={id} />;
}
