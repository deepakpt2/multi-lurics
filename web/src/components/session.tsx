"use client";

import { useEffect, useState } from "react";
import { api, call, type User } from "@/api/client";
import { usePathname } from "@/i18n/navigation";

/** Current user from the session cookie: `undefined` while loading, `null` when signed out.
 *  Re-checked on every navigation so the header follows sign-in and sign-out.
 *  This only shapes the UI; the server authorizes every request. */
export function useSession() {
  const pathname = usePathname();
  const [user, setUser] = useState<User | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    call(api.GET("/api/v1/me")).then(
      (u) => live && setUser(u),
      () => live && setUser(null),
    );
    return () => {
      live = false;
    };
  }, [pathname]);
  const has = (role: "moderator" | "admin") => !!user?.roles.includes("admin") || !!user?.roles.includes(role);
  return { user, isModerator: has("moderator"), isAdmin: has("admin") };
}
