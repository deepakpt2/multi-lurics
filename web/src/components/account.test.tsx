import { describe, expect, it } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { push } from "@/test/setup";
import { AccountView } from "./account";

const user = { id: "u1", email: "ilaiyaraaja@example.com", display_name: "இளையராஜா", roles: ["moderator"], created_at: "2026-03-01T00:00:00Z" };

describe("AccountView (protected endpoint)", () => {
  it("happy path: loads the signed-in user via the session cookie", async () => {
    const { calls } = mockFetch({ "GET /api/v1/me": () => json(200, user) });
    renderIntl(<AccountView />);
    expect(screen.getByRole("status")).toHaveTextContent("Loading…");
    const name = await screen.findByText("இளையராஜா");
    expect(name).toHaveAttribute("translate", "no");
    expect(name).toHaveAttribute("dir", "auto");
    expect(screen.getByText("Moderator")).toBeInTheDocument();
    expect(calls[0].credentials).toBe("same-origin");
  });

  it("unauthorized: shows signed-out state with sign-in link", async () => {
    mockFetch({ "GET /api/v1/me": () => problem(401, "unauthenticated") });
    renderIntl(<AccountView />);
    expect(await screen.findByText("You are signed out")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/login");
  });

  it("server failure: shows error and retries", async () => {
    let n = 0;
    mockFetch({ "GET /api/v1/me": () => (n++ === 0 ? problem(500, "internal") : json(200, user)) });
    renderIntl(<AccountView />);
    expect(await screen.findByText("Something went wrong on our side. Please try again.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("இளையராஜா")).toBeInTheDocument();
  });

  it("sign out revokes the session and navigates to login", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/me": () => json(200, user),
      "POST /api/v1/auth/logout": () => new Response(null, { status: 204 }),
    });
    renderIntl(<AccountView />);
    await userEvent.click(await screen.findByRole("button", { name: "Sign out" }));
    await waitFor(() => expect(push).toHaveBeenCalledWith("/login"));
    expect(calls.at(-1)?.headers.get("x-requested-with")).toBe("fetch");
  });
});
