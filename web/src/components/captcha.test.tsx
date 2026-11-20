import { afterEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, renderIntl } from "@/test/utils";
import { LoginForm } from "./auth-forms";

afterEach(() => {
  delete window.turnstile;
});

describe("bot check on the sign-in form", () => {
  it("holds the button until the challenge is answered, sends the answer once, then asks for a new challenge", async () => {
    let answer: (t: string) => void = () => undefined;
    const reset = vi.fn();
    window.turnstile = {
      render: (_el, o) => {
        expect(o.sitekey).toBe("site-key");
        answer = o.callback as (t: string) => void;
        return "w1";
      },
      reset,
      remove: vi.fn(),
    };
    const { calls } = mockFetch({
      "GET /api/v1/captcha": () => json(200, { enabled: true, site_key: "site-key" }),
      "POST /api/v1/auth/login": () => json(422, { type: "x", title: "x", status: 422, code: "validation_failed", errors: [{ field: "captcha", code: "captcha_failed" }] }),
    });
    renderIntl(<LoginForm />);
    const submit = await screen.findByRole("button", { name: "Sign in" });
    await waitFor(() => expect(submit).toBeDisabled());
    await userEvent.type(screen.getByLabelText("Email"), "a@example.com");
    await userEvent.type(screen.getByLabelText("Password"), "secret-password");
    await waitFor(() => expect(window.turnstile && answer).toBeTruthy());
    act(() => answer("token-1"));
    await waitFor(() => expect(submit).toBeEnabled());
    await userEvent.click(submit);
    expect(await screen.findByText("The check was not accepted. Please try again.")).toBeInTheDocument();
    const sent = (await calls.find((c) => c.method === "POST")!.json()) as Record<string, unknown>;
    expect(sent.captcha_token).toBe("token-1");
    expect(reset).toHaveBeenCalledWith("w1");
    // The used answer is gone: a new one is needed.
    expect(submit).toBeDisabled();
  });
});
