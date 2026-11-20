import { Album, BookOpenText, Languages, Mic2, Music, Music2, PenLine, Sparkles, Tag, Users, Waves, type LucideIcon } from "lucide-react";
import type { FacetKey } from "@/lib/browse";

/** How each kind of metadata looks. Which kinds exist comes from the API; this only dresses them. */
export const FACET_ICON: Record<FacetKey, LucideIcon> = {
  album: Album,
  artist: Users,
  singer: Mic2,
  lyricist: PenLine,
  composer: Music2,
  raga: Waves,
  deity: Sparkles,
  religion: BookOpenText,
  category: Music,
  language: Languages,
  tag: Tag,
};

/** Facets whose value is a fixed code the UI translates, rather than a name. */
export const CODED: ReadonlySet<FacetKey> = new Set(["religion", "category"]);
