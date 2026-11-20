"use client";

import { useEffect } from "react";
import { useRouter } from "@/i18n/navigation";
import { useSession } from "./session";

/** Sign-in and registration pages are for visitors: someone already signed in is sent on to browse. */
export function RedirectIfSignedIn({ children }: { children: React.ReactNode }) {
  const { user } = useSession();
  const router = useRouter();
  useEffect(() => {
    if (user) router.replace("/browse");
  }, [user, router]);
  return user ? null : <>{children}</>;
}

/** Shown only once we know the visitor is signed out (nothing while the session loads, so no flash). */
export function GuestOnly({ children }: { children: React.ReactNode }) {
  const { user } = useSession();
  return user === null ? <>{children}</> : null;
}
