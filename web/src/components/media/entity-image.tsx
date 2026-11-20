"use client";

import { useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { Building2, Camera, Disc3, Flower2, Music2, Pencil, User, type LucideIcon } from "lucide-react";
import { Link } from "@/i18n/navigation";
import { mediaSources, type Image, type ImageEntity } from "@/lib/images";
import { useSession } from "../session";
import { ImageUpload } from "./image-upload";

const PLACEHOLDER = {
  artist: User,
  album: Disc3,
  label: Building2,
  publisher: Building2,
  genre: Music2,
  raga: Music2,
  deity: Flower2,
} as const satisfies Record<ImageEntity, LucideIcon>;

/**
 * A small square picture that goes with a name. Without one it shows a placeholder for the kind of
 * thing it is, plus a small edit button (when `entity` is given) to add a real photo. A photo waits
 * for review before it replaces the placeholder, unless the person may publish directly.
 */
export function EntityImage({
  type,
  id,
  imageId,
  name,
  size = 48,
}: {
  type: ImageEntity;
  /** Needed to offer the edit button. */
  id?: string;
  imageId?: string | null;
  name: string;
  size?: number;
}) {
  const t = useTranslations("images");
  const [open, setOpen] = useState(false);
  const [shown, setShown] = useState<string | null | undefined>(imageId);
  useEffect(() => setShown(imageId), [imageId]);
  const Icon = PLACEHOLDER[type];
  return (
    <span className="entity-image" style={{ inlineSize: size, blockSize: size }}>
      {shown ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img {...mediaSources(shown, size)} alt="" width={size} height={size} loading="lazy" decoding="async" />
      ) : (
        <span className="entity-image-ph" role="img" aria-label={t("placeholder", { name })} data-type={type}>
          <Icon size={Math.round(size * 0.5)} aria-hidden="true" />
        </span>
      )}
      {id && (
        <button type="button" className="entity-image-edit" aria-label={shown ? t("change", { name }) : t("add", { name })} title={shown ? t("change", { name }) : t("add", { name })} onClick={() => setOpen(true)}>
          {shown ? <Pencil size={12} aria-hidden="true" /> : <Camera size={12} aria-hidden="true" />}
        </button>
      )}
      {open && id && <ImageDialog type={type} id={id} name={name} onClose={() => setOpen(false)} onPublished={(img) => setShown(img)} />}
    </span>
  );
}

function ImageDialog({ type, id, name, onClose, onPublished }: { type: ImageEntity; id: string; name: string; onClose: () => void; onPublished: (imageId: string) => void }) {
  const t = useTranslations("images");
  const ref = useRef<HTMLDialogElement>(null);
  const { user } = useSession();
  const [done, setDone] = useState<Image>();
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (typeof d.showModal === "function") d.showModal();
    else d.setAttribute("open", "");
  }, []);
  return (
    <dialog ref={ref} className="image-dialog" aria-label={t("dialogTitle", { name })} onClose={onClose} onClick={(e) => e.target === ref.current && ref.current?.close()}>
      <h2>{t("dialogTitle", { name })}</h2>
      {done ? (
        <div role="status">
          <p>{done.status === "approved" ? t("published") : t("pending")}</p>
          <button type="button" className="button" onClick={() => ref.current?.close()}>
            {t("close")}
          </button>
        </div>
      ) : user === null ? (
        <p>
          <Link href="/login">{t("signIn")}</Link>
        </p>
      ) : (
        <ImageUpload
          entity={{ type, id }}
          onCancel={() => ref.current?.close()}
          onUploaded={(img) => {
            setDone(img);
            if (img.status === "approved") onPublished(img.id);
          }}
        />
      )}
    </dialog>
  );
}
