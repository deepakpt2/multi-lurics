import { getTranslations, setRequestLocale } from "next-intl/server";
import { UsersAdmin } from "@/components/admin/users";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("admin");
  return (
    <>
      <h1>{t("usersTitle")}</h1>
      <UsersAdmin />
    </>
  );
}
