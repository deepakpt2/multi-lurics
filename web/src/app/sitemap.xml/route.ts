import { fetchSitemap, siteUrl, xmlEscape } from "@/lib/seo";

export const dynamic = "force-dynamic";


/** Index of sitemap files, one per page of the API's feed. */
export async function GET() {
  const { pages } = await fetchSitemap(0);
  const files = Array.from({ length: Math.max(pages, 1) }, (_, n) => `  <sitemap><loc>${xmlEscape(`${siteUrl()}/sitemaps/${n}.xml`)}</loc></sitemap>`);
  const xml = `<?xml version="1.0" encoding="UTF-8"?>\n<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${files.join("\n")}\n</sitemapindex>\n`;
  return new Response(xml, { headers: { "content-type": "application/xml; charset=utf-8" } });
}
