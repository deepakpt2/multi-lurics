import { getRequestConfig } from "next-intl/server";
import { hasLocale } from "next-intl";
import { withFallback } from "./messages";
import { routing } from "./routing";

export default getRequestConfig(async ({ requestLocale }) => {
  const requested = await requestLocale;
  const locale = hasLocale(routing.locales, requested) ? requested : routing.defaultLocale;
  const messages = (await import(`../../messages/${locale}.json`)).default;
  // Malayalam is translated page by page; the rest of it reads in English until it is.
  return { locale, messages: locale === "ml" ? withFallback((await import("../../messages/en.json")).default, messages) : messages };
});
