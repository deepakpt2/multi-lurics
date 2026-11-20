import "@testing-library/jest-dom/vitest";
import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import { createElement } from "react";

export const push = vi.fn();
export const replace = vi.fn();

vi.mock("@/i18n/navigation", () => ({
  Link: ({ href, children, ...rest }: { href: string; children: React.ReactNode; locale?: string }) => {
    delete rest.locale;
    return createElement("a", { href, ...rest }, children);
  },
  useRouter: () => ({ push, replace }),
  usePathname: () => "/",
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  push.mockReset();
  replace.mockReset();
  if (typeof window !== "undefined") window.location.hash = "";
});
