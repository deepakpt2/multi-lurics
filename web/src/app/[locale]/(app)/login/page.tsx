import { getTranslations, setRequestLocale } from "next-intl/server";
import { LoginForm } from "@/components/auth-forms";
import { RedirectIfSignedIn } from "@/components/guest-only";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("login");
  return (
    <>
      <h1>{t("title")}</h1>
      <RedirectIfSignedIn>
        <LoginForm />
      </RedirectIfSignedIn>
    </>
  );
}
