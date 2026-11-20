"use client";

import * as Dropdown from "@radix-ui/react-dropdown-menu";
import { Camera, HandHeart, Languages, LogIn, PenLine, Type } from "lucide-react";
import { useTranslations } from "next-intl";
import { useState } from "react";
import { Link } from "@/i18n/navigation";
import type { Block } from "@/lib/contribute";
import { AddLyricsDialog } from "../contribute/add-lyrics";
import { useSession } from "../session";

type Target = { kind: "song"; id: string; title?: string } | { kind: "artist" | "album"; id: string };

/**
 * Every page has a way to add to it. Anyone signed in may add a translation or transliteration, lyrics in
 * another script, and details that are missing; changing what is already there stays with the person who
 * created it and the moderators. Signed out, the button asks to sign in.
 */
export function ContributeButton({ target }: { target: Target }) {
  const t = useTranslations("public.contribute");
  const { user } = useSession();
  const [adding, setAdding] = useState<Block["type"] | null>(null);
  if (user === undefined) return null;
  if (user === null)
    return (
      <Link href="/login" className="button">
        <LogIn size={16} aria-hidden="true" /> {t("signIn")}
      </Link>
    );
  const details = target.kind === "song" ? `/compositions/${target.id}/complete` : target.kind === "artist" ? `/artists/${target.id}/complete` : `/albums/${target.id}/complete`;
  const Link_ = ({ href, icon: I, children }: { href: string; icon: typeof Type; children: React.ReactNode }) => (
    <Dropdown.Item asChild>
      <Link href={href} className="menu-item">
        <I size={16} aria-hidden="true" />
        {children}
      </Link>
    </Dropdown.Item>
  );
  const Act = ({ kind, icon: I, children }: { kind: Block["type"]; icon: typeof Type; children: React.ReactNode }) => (
    <Dropdown.Item className="menu-item" onSelect={() => setAdding(kind)}>
      <I size={16} aria-hidden="true" />
      {children}
    </Dropdown.Item>
  );
  return (
    <>
      <Dropdown.Root>
        <Dropdown.Trigger className="button primary">
          <HandHeart size={16} aria-hidden="true" /> {t("button")}
        </Dropdown.Trigger>
        <Dropdown.Portal>
          <Dropdown.Content className="menu" align="start" sideOffset={8}>
            <Dropdown.Label className="menu-label">{t("label")}</Dropdown.Label>
            {target.kind === "song" && (
              <>
                <Act kind="translation" icon={Languages}>
                  {t("translation")}
                </Act>
                <Act kind="transliteration" icon={Type}>
                  {t("transliteration")}
                </Act>
                <Act kind="original" icon={PenLine}>
                  {t("original")}
                </Act>
              </>
            )}
            <Link_ href={details} icon={PenLine}>
              {t("details")}
            </Link_>
            {target.kind !== "song" && (
              <p className="menu-note muted small">
                <Camera size={14} aria-hidden="true" /> {t("photoHint")}
              </p>
            )}
          </Dropdown.Content>
        </Dropdown.Portal>
      </Dropdown.Root>
      {adding && target.kind === "song" && <AddLyricsDialog songId={target.id} title={target.title} type={adding} onClose={() => setAdding(null)} />}
    </>
  );
}
