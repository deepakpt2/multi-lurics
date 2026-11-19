import { describe, expect, it } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { push } from "@/test/setup";
import { LoginForm, RegisterForm, ResetPasswordForm, VerifyEmail } from "./auth-forms";

describe("auth forms", () => {
  it("register shows server-side field errors next to fields", async () => {
    mockFetch({
      "POST /api/v1/auth/register": () =>
        problem(422, "validation_failed", { errors: [{ field: "email", code: "invalid_email" }, { field: "password", code: "too_short" }] }),
    });
    renderIntl(<RegisterForm />);
    await userEvent.click(screen.getByRole("button", { name: "Create account" }));
    expect(await screen.findByText("Enter a valid email address.")).toBeInTheDocument();
    expect(screen.getByText("This is too short.")).toBeInTheDocument();
    expect(screen.getByLabelText("Email")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Display name")).not.toHaveAttribute("aria-invalid");
  });

  it("register sends the UI locale and shows neutral success", async () => {
    const { calls } = mockFetch({ "POST /api/v1/auth/register": () => new Response(null, { status: 202 }) });
    renderIntl(<RegisterForm />, "ar");
    await userEvent.type(screen.getByLabelText("البريد الإلكتروني"), "a@b.co");
    await userEvent.click(screen.getByRole("button", { name: "إنشاء الحساب" }));
    expect(await screen.findByRole("status")).toHaveTextContent("تحقّق من بريدك الإلكتروني");
    expect(await calls[0].json()).toMatchObject({ email: "a@b.co", locale: "ar" });
  });

  it("login failure shows the localized problem; success navigates", async () => {
    let ok = false;
    mockFetch({
      "POST /api/v1/auth/login": () =>
        ok
          ? json(200, { id: "u", email: "a@b.co", display_name: "A", roles: [], created_at: "2026-01-01T00:00:00Z" })
          : problem(401, "invalid_credentials"),
    });
    renderIntl(<LoginForm />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Email or password is incorrect.");
    ok = true;
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(push).toHaveBeenCalledWith("/account"));
  });

  it("disables submit while in flight (no double submit)", async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const { spy } = mockFetch({ "POST /api/v1/auth/login": () => new Response(null, { status: 401 }) });
    spy.mockImplementation(async () => (await gate, new Response(null, { status: 401 })));
    renderIntl(<LoginForm />);
    const btn = screen.getByRole("button", { name: "Sign in" });
    await userEvent.click(btn);
    expect(screen.getByRole("button", { name: "Please wait…" })).toBeDisabled();
    release();
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(spy).toHaveBeenCalledOnce();
  });

  it("verify-email reads the token from the URL fragment and submits it once", async () => {
    window.location.hash = "#token=abc123";
    const { calls } = mockFetch({ "POST /api/v1/auth/verify-email": () => new Response(null, { status: 204 }) });
    renderIntl(<VerifyEmail />);
    expect(await screen.findByText(/Your email is verified/)).toBeInTheDocument();
    expect(calls).toHaveLength(1);
    expect(await calls[0].json()).toEqual({ token: "abc123" });
  });

  it("verify-email shows expired-link error", async () => {
    window.location.hash = "#token=used";
    mockFetch({ "POST /api/v1/auth/verify-email": () => problem(400, "invalid_token") });
    renderIntl(<VerifyEmail />);
    expect(await screen.findByRole("alert")).toHaveTextContent("This link is invalid or has expired.");
  });

  it("reset-password without a token explains instead of submitting", async () => {
    const { spy } = mockFetch({});
    renderIntl(<ResetPasswordForm />);
    expect(await screen.findByText("This reset link is incomplete. Request a new one.")).toBeInTheDocument();
    expect(spy).not.toHaveBeenCalled();
  });
});
