import { describe, expect, it } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { replace } from "@/test/setup";
import { GuestOnly, RedirectIfSignedIn } from "./guest-only";

const me = { id: "u1", email: "a@example.com", display_name: "A", roles: [], email_verified: true };

describe("guest-only UI", () => {
  it("shows sign-in and register to a signed-out visitor", async () => {
    mockFetch({ "GET /api/v1/me": () => problem(401, "unauthenticated") });
    renderIntl(<GuestOnly><p>Sign in</p></GuestOnly>);
    expect(await screen.findByText("Sign in")).toBeInTheDocument();
  });

  it("hides them from someone signed in", async () => {
    const { calls } = mockFetch({ "GET /api/v1/me": () => json(200, me) });
    renderIntl(<GuestOnly><p>Sign in</p></GuestOnly>);
    await new Promise((r) => setTimeout(r, 50));
    expect(calls.length).toBe(1);
    expect(screen.queryByText("Sign in")).not.toBeInTheDocument();
  });

  it("sends a signed-in visitor away from the sign-in form", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me) });
    renderIntl(<RedirectIfSignedIn><p>form</p></RedirectIfSignedIn>);
    await waitFor(() => expect(replace).toHaveBeenCalledWith("/browse"));
    expect(screen.queryByText("form")).not.toBeInTheDocument();
  });
});
