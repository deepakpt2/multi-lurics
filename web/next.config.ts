import type { NextConfig } from "next";
import createNextIntlPlugin from "next-intl/plugin";

// Read at BUILD time: Next bakes rewrites into the build output.
const apiOrigin = process.env.API_ORIGIN ?? "http://127.0.0.1:8787";

const nextConfig: NextConfig = {
  // Browser talks to one origin; Next proxies the versioned API. Keeps the
  // session cookie first-party and means the API never needs CORS.
  async rewrites() {
    return [{ source: "/api/v1/:path*", destination: `${apiOrigin}/api/v1/:path*` }];
  },
  async headers() {
    return [
      {
        source: "/:path*",
        headers: [
          { key: "Referrer-Policy", value: "no-referrer" },
          { key: "X-Content-Type-Options", value: "nosniff" },
          { key: "X-Frame-Options", value: "DENY" },
        ],
      },
    ];
  },
};

export default createNextIntlPlugin("./src/i18n/request.ts")(nextConfig);
