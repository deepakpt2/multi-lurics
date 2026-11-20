import { getTranslations, setRequestLocale } from "next-intl/server";
import { RegisterForm } from "@/components/auth-forms";
import { RedirectIfSignedIn } from "@/components/guest-only";

export default async function Page({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations("register");
  return (
    <>
      <h1>{t("title")}</h1>
      <RedirectIfSignedIn>
        <RegisterForm />
      </RedirectIfSignedIn>
    </>
  );
}
