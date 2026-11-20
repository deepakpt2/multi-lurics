import { ExternalLink } from "lucide-react";
import { getTranslations } from "next-intl/server";
import { embedUrl, watchUrl } from "@/lib/youtube";

/**
 * The song's YouTube video. Built from the stored id alone, so a change in YouTube's embed address is a
 * one-line fix in `lib/youtube.ts`. Loaded lazily, from the privacy-enhanced domain.
 */
export async function VideoEmbed({ id, title }: { id: string; title: string }) {
  const t = await getTranslations("public.video");
  return (
    <section aria-label={t("label")} className="video">
      <h2>{t("title")}</h2>
      <div className="video-frame">
        <iframe
          src={embedUrl(id)}
          title={t("frameTitle", { title })}
          loading="lazy"
          allow="accelerometer; encrypted-media; gyroscope; picture-in-picture; web-share"
          allowFullScreen
          referrerPolicy="strict-origin-when-cross-origin"
        />
      </div>
      <p className="muted small">
        <a href={watchUrl(id)} target="_blank" rel="noopener noreferrer">
          <ExternalLink size={14} aria-hidden="true" /> {t("openOnYoutube")}
        </a>
      </p>
    </section>
  );
}
