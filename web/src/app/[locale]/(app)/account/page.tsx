import { getTranslations, setRequestLocale } from "next-intl/server";
import { AccountView } from "@/components/account";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("account");
  return (
    <>
      <h1>{t("title")}</h1>
      <AccountView />
    </>
  );
}
