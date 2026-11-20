import { fetchSitemap, pageUrl, xmlEscape } from "@/lib/seo";

export const dynamic = "force-dynamic";


/** One sitemap file: each public page once, at its canonical URL, with `hreflang` alternates for the locales that have content. */
export async function GET(_req: Request, { params }: { params: Promise<{ file: string }> }) {
  const { file } = await params;
  const m = /^(\d{1,6})\.xml$/.exec(file);
  if (!m) return new Response("not found", { status: 404 });
  const { items } = await fetchSitemap(Number(m[1]));
  const urls = items.map((i) => {
    const alt = i.alternates.map((a) => `<xhtml:link rel="alternate" hreflang="${xmlEscape(a.locale)}" href="${xmlEscape(pageUrl(a, i.scope))}"/>`);
    alt.push(`<xhtml:link rel="alternate" hreflang="x-default" href="${xmlEscape(pageUrl(i.canonical, i.scope))}"/>`);
    return `  <url><loc>${xmlEscape(pageUrl(i.canonical, i.scope))}</loc><lastmod>${i.lastmod}</lastmod>${alt.join("")}</url>`;
  });
  const xml = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">\n${urls.join("\n")}\n</urlset>\n`;
  return new Response(xml, { headers: { "content-type": "application/xml; charset=utf-8" } });
}
