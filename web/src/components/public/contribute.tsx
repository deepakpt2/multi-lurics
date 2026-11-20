"use client";

import * as Dropdown from "@radix-ui/react-dropdown-menu";
import { HandHeart, Languages, PenLine, Type, LogIn, Camera } from "lucide-react";
import { useTranslations } from "next-intl";
import { Link } from "@/i18n/navigation";
import { useSession } from "../session";

type Target =
  | { kind: "song"; id: string; recordingId?: string; sourceId?: string }
  | { kind: "recording"; id: string; recordingId?: string; sourceId?: string }
  | { kind: "artist" | "album"; id: string };

/**
 * Every page has a way to add to it. Anyone signed in may add a translation or transliteration, lyrics in
 * another script, and details that are missing; changing what is already there stays with the person who
 * created it and the moderators. Signed out, the button asks to sign in.
 */
export function ContributeButton({ target }: { target: Target }) {
  const t = useTranslations("public.contribute");
  const { user } = useSession();
  if (user === undefined) return null;
  if (user === null)
    return (
      <Link href="/login" className="button">
        <LogIn size={16} aria-hidden="true" /> {t("signIn")}
      </Link>
    );
  const lyricHref = (type: "original" | "translation" | "transliteration") =>
    `/recordings/${target.kind === "song" || target.kind === "recording" ? target.recordingId : ""}/lyrics/new?type=${type}${
      (target.kind === "song" || target.kind === "recording") && target.sourceId && type !== "original" ? `&source=${target.sourceId}` : ""
    }`;
  const details = target.kind === "song" ? `/compositions/${target.id}/complete` : target.kind === "artist" ? `/artists/${target.id}/complete` : target.kind === "album" ? `/albums/${target.id}/complete` : null;
  const lyrics = (target.kind === "song" || target.kind === "recording") && !!target.recordingId;
  const Item = ({ href, icon: I, children }: { href: string; icon: typeof Type; children: React.ReactNode }) => (
    <Dropdown.Item asChild>
      <Link href={href} className="menu-item">
        <I size={16} aria-hidden="true" />
        {children}
      </Link>
    </Dropdown.Item>
  );
  return (
    <Dropdown.Root>
      <Dropdown.Trigger className="button primary">
        <HandHeart size={16} aria-hidden="true" /> {t("button")}
      </Dropdown.Trigger>
      <Dropdown.Portal>
        <Dropdown.Content className="menu" align="start" sideOffset={8}>
          <Dropdown.Label className="menu-label">{t("label")}</Dropdown.Label>
          {lyrics && (
            <>
              <Item href={lyricHref("translation")} icon={Languages}>
                {t("translation")}
              </Item>
              <Item href={lyricHref("transliteration")} icon={Type}>
                {t("transliteration")}
              </Item>
              <Item href={lyricHref("original")} icon={PenLine}>
                {t("original")}
              </Item>
            </>
          )}
          {details && (
            <Item href={details} icon={PenLine}>
              {t("details")}
            </Item>
          )}
          {(target.kind === "artist" || target.kind === "album") && (
            <p className="menu-note muted small">
              <Camera size={14} aria-hidden="true" /> {t("photoHint")}
            </p>
          )}
        </Dropdown.Content>
      </Dropdown.Portal>
    </Dropdown.Root>
  );
}
