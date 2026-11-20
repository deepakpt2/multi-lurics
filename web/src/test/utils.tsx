import { render } from "@testing-library/react";
import { NextIntlClientProvider } from "next-intl";
import { vi } from "vitest";
import en from "../../messages/en.json";
import ar from "../../messages/ar.json";

const messages = { en, ar };

export function renderIntl(ui: React.ReactElement, locale: "en" | "ar" = "en") {
  return render(
    <NextIntlClientProvider locale={locale} messages={messages[locale]} timeZone="UTC">
      {ui}
    </NextIntlClientProvider>,
  );
}

const json = (status: number, body: unknown, type = "application/json") =>
  new Response(body === undefined ? null : JSON.stringify(body), { status, headers: { "content-type": type } });

export const problem = (status: number, code: string, extra: Record<string, unknown> = {}) =>
  json(status, { type: `/problems/${code}`, title: code, status, code, request_id: "req-1", ...extra }, "application/problem+json");

/** Routes fetch calls by "METHOD path" → response factory; records requests. */
export function mockFetch(routes: Record<string, () => Response>) {
  const calls: Request[] = [];
  const spy = vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    const req = input as Request;
    calls.push(req);
    const key = `${req.method} ${new URL(req.url, "http://localhost").pathname}`;
    const route = routes[key];
    if (!route) throw new Error(`unmocked fetch: ${key}`);
    return route();
  });
  return { spy, calls };
}

export { json };

/** Chooses a language the way a person does: types some of its name, then picks it from the suggestions. */
export async function pickLanguage(root: { findByLabelText: (t: string) => Promise<HTMLElement> }, typed: string, name: RegExp) {
  const { default: userEvent } = await import("@testing-library/user-event");
  const { within, waitFor } = await import("@testing-library/react");
  await userEvent.type(await root.findByLabelText("Language"), typed);
  const list = await waitFor(() => {
    const el = document.querySelector<HTMLElement>(".language-list");
    if (!el) throw new Error("no suggestions yet");
    return el;
  });
  await userEvent.click(await within(list).findByRole("option", { name }));
}
