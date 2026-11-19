"use client";

import { useLocale } from "next-intl";
import { setDisplayLanguage } from "@/api/client";

/** Renders before any page content so the first API read already carries the reader's language. */
export function LocaleSync() {
  setDisplayLanguage(useLocale());
  return null;
}
