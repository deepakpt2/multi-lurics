"use client";

import { ErrorState } from "@/components/states";

/** Route-level boundary: unexpected render errors get the same localized error UI. */
export default function RouteError({ error, reset }: { error: Error; reset: () => void }) {
  return <ErrorState error={error} onRetry={reset} />;
}
