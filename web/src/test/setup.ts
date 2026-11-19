import "@testing-library/jest-dom/vitest";
import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import { createElement } from "react";

export const push = vi.fn();

vi.mock("@/i18n/navigation", () => ({
  Link: ({ href, children, ...rest }: { href: string; children: React.ReactNode; locale?: string }) => {
    delete rest.locale;
    return createElement("a", { href, ...rest }, children);
  },
  useRouter: () => ({ push }),
  usePathname: () => "/",
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  push.mockReset();
  window.location.hash = "";
});
