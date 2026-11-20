import type { MetadataRoute } from "next";
import { siteUrl } from "@/lib/seo";

// Read SITE_URL at request time, not at build time: the image is built once and configured at runtime.
export const dynamic = "force-dynamic";

/** Public pages are indexable; the account, editing and id-based management screens are not. */
export default function robots(): MetadataRoute.Robots {
  return {
    rules: {
      userAgent: "*",
      allow: "/",
      disallow: [
        "/api/",
        "/*/account",
        "/*/login",
        "/*/register",
        "/*/forgot-password",
        "/*/reset-password",
        "/*/verify-email",
        "/*/moderation",
        "/*/contributions",
        "/*/names",
        "/*/search",
        "/*/compositions",
        "/*/recordings",
        "/*/artists",
        "/*/albums",
        // Query-string variants are never separate pages.
        "/*?*",
      ],
    },
    sitemap: `${siteUrl()}/sitemap.xml`,
    host: siteUrl(),
  };
}
