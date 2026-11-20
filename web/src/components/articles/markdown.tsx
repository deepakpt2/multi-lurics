"use client";

import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { safeImageSrc } from "@/lib/articles";

// The page's own title is the h1, so the text's headings start one level lower.
const heading = (level: 2 | 3 | 4 | 5 | 6) => {
  const Tag = `h${level}` as const;
  const H = ({ children }: { children?: React.ReactNode }) => <Tag>{children}</Tag>;
  return H;
};

const components: Components = {
  h1: heading(2),
  h2: heading(3),
  h3: heading(4),
  h4: heading(5),
  h5: heading(6),
  h6: heading(6),
  a: ({ href, children }) =>
    href?.startsWith("http") ? (
      <a href={href} target="_blank" rel="noopener noreferrer">
        {children}
      </a>
    ) : (
      <a href={href}>{children}</a>
    ),
  img: ({ src, alt }) =>
    typeof src === "string" && safeImageSrc(src) ? (
      // eslint-disable-next-line @next/next/no-img-element
      <img src={src} alt={alt ?? ""} loading="lazy" decoding="async" />
    ) : null,
};

/**
 * Markdown written elsewhere, shown in the site's own style. Raw HTML in the text is ignored (it is
 * never inserted into the page), links with unsafe schemes are dropped, and pictures may only come
 * from this site or an https address.
 */
export function ArticleBody({ markdown }: { markdown: string }) {
  return (
    <div className="article-body" dir="auto">
      <ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml components={components}>
        {markdown}
      </ReactMarkdown>
    </div>
  );
}
