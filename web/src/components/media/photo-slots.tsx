"use client";

import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { api, call } from "@/api/client";
import type { ImageEntity } from "@/lib/images";
import { Name } from "../localization/localized";
import { EntityImage } from "./entity-image";
import type { components } from "@/api/schema";

type Display = components["schemas"]["Display"];
type Slot = { type: ImageEntity; id: string; display: Display };

const MAX_SLOTS = 6;

/**
 * Offered while adding lyrics: the artists and albums of this recording that have no photo yet, each
 * with the usual placeholder and edit button. Entirely optional and independent of saving the lyrics;
 * anything added goes to review like any other photo.
 */
export function PhotoSlots({ recordingId }: { recordingId: string }) {
  const t = useTranslations("images");
  const [slots, setSlots] = useState<Slot[]>([]);
  useEffect(() => {
    let live = true;
    (async () => {
      const rec = await call(api.GET("/api/v1/recordings/{id}", { params: { path: { id: recordingId } } }));
      const seen = new Set<string>();
      const found: Slot[] = [];
      for (const c of rec.credits) {
        if (!seen.has(c.artist_id)) {
          seen.add(c.artist_id);
          found.push({ type: "artist", id: c.artist_id, display: c.artist_display });
        }
      }
      for (const a of rec.albums) found.push({ type: "album", id: a.album_id, display: a.display });
      const states = await Promise.all(
        found.slice(0, 12).map((s) => call(api.GET("/api/v1/entities/{entity_type}/{entity_id}/images", { params: { path: { entity_type: s.type, entity_id: s.id } } })).catch(() => null)),
      );
      if (live) setSlots(found.slice(0, 12).filter((_, i) => states[i] && !states[i].current && !states[i].mine_pending).slice(0, MAX_SLOTS));
    })().catch(() => undefined);
    return () => {
      live = false;
    };
  }, [recordingId]);
  if (slots.length === 0) return null;
  return (
    <section className="photos-section" aria-labelledby="photos-title">
      <h2 id="photos-title">{t("optionalTitle")}</h2>
      <p className="muted">{t("optionalIntro")}</p>
      <ul className="photo-slots">
        {slots.map((s) => (
          <li key={`${s.type}:${s.id}`}>
            <EntityImage type={s.type} id={s.id} imageId={null} name={s.display.text} size={56} />
            <span>
              <Name display={s.display} />
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
