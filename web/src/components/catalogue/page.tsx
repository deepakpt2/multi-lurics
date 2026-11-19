import { getTranslations, setRequestLocale } from "next-intl/server";
import type { Kind } from "./common";
import { CatalogueView, type View } from "./views";

type PageProps = {
  params: Promise<{ locale: string; id?: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
};

/** Route module factory: each catalogue route file is one line. Detail views render their own heading. */
export function cataloguePage(kind: Kind, view: View) {
  return async function CataloguePage({ params, searchParams }: PageProps) {
    const { locale, id = "" } = await params;
    setRequestLocale(locale);
    const t = await getTranslations("catalogue");
    const query = Object.fromEntries(
      Object.entries(await searchParams).filter((e): e is [string, string] => typeof e[1] === "string"),
    );
    return (
      <>
        {view !== "detail" && <h1>{t(`${kind}.${view}Title`)}</h1>}
        <CatalogueView kind={kind} view={view} id={id} query={query} />
      </>
    );
  };
}
