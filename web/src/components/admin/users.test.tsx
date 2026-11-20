import { describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { SiteHeader } from "../site-header";
import { UsersAdmin } from "./users";

vi.mock("next/navigation", () => ({ useSearchParams: () => new URLSearchParams(""), useRouter: () => ({ push: vi.fn() }), usePathname: () => "/" }));

const me = (roles: string[]) => ({ id: "me", email: "me@example.com", display_name: "Me", roles, created_at: "2026-03-01T00:00:00Z" });
const row = (id: string, name: string, roles: string[] = [], status = "active") => ({ id, email: `${name}@example.com`, display_name: name, status, roles, created_at: "2026-03-01T00:00:00Z" });
const links = () => screen.queryAllByRole("link").map((a) => a.textContent);

describe("header follows the session", () => {
  it("signed out: sign in and register, nothing restricted, no language switcher", async () => {
    mockFetch({ "GET /api/v1/me": () => problem(401, "unauthenticated") });
    renderIntl(<SiteHeader />);
    expect(await screen.findByRole("link", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Create account" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Moderation" })).toBeNull();
    expect(screen.queryByRole("navigation", { name: "Language" })).toBeNull();
    expect(screen.queryByText("العربية")).toBeNull();
  });

  it("signed in: no sign-in links; a plain member has no moderation or users entry", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me([])) });
    renderIntl(<SiteHeader />);
    await userEvent.click(await screen.findByRole("button", { name: "Menu" }));
    expect(await screen.findByRole("menuitem", { name: "Account" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Sign out" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Moderation" })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: "Users" })).toBeNull();
    expect(links()).not.toContain("Sign in");
    expect(links()).not.toContain("Create account");
  });

  it("moderators see Moderation, only admins see Users", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me(["moderator"])) });
    const first = renderIntl(<SiteHeader />);
    await userEvent.click(await screen.findByRole("button", { name: "Menu" }));
    expect(await screen.findByRole("menuitem", { name: "Moderation" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Users" })).toBeNull();
    first.unmount();
    mockFetch({ "GET /api/v1/me": () => json(200, me(["admin"])) });
    renderIntl(<SiteHeader />);
    await userEvent.click(await screen.findByRole("button", { name: "Menu" }));
    expect(await screen.findByRole("menuitem", { name: "Users" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Moderation" })).toBeInTheDocument();
  });
});

describe("UsersAdmin", () => {
  it("lists users, grants and removes moderator, never offers actions on yourself", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/me": () => json(200, me(["admin"])),
      "GET /api/v1/admin/users": () => json(200, { items: [row("me", "Me", ["admin"]), row("u1", "Asha")], next_cursor: null }),
      "PUT /api/v1/admin/users/u1/roles/moderator": () => json(200, row("u1", "Asha", ["moderator"])),
      "DELETE /api/v1/admin/users/u1/roles/moderator": () => json(200, row("u1", "Asha")),
    });
    renderIntl(<UsersAdmin />);
    const rows = await screen.findAllByTestId("user-row");
    expect(within(rows[0]).queryByRole("button")).toBeNull();
    await userEvent.click(within(rows[1]).getByRole("button", { name: "Make moderator" }));
    await userEvent.click(await within(rows[1]).findByRole("button", { name: "Remove moderator" }));
    await within(rows[1]).findByRole("button", { name: "Make moderator" });
    expect(calls.filter((c) => c.method !== "GET").map((c) => c.method)).toEqual(["PUT", "DELETE"]);
  });

  it("disables an account and shows server refusals", async () => {
    mockFetch({
      "GET /api/v1/me": () => json(200, me(["admin"])),
      "GET /api/v1/admin/users": () => json(200, { items: [row("u1", "Asha"), row("u2", "Ravi")], next_cursor: null }),
      "POST /api/v1/admin/users/u1/disable": () => json(200, row("u1", "Asha", [], "disabled")),
      "POST /api/v1/admin/users/u2/disable": () => problem(403, "forbidden"),
    });
    renderIntl(<UsersAdmin />);
    const rows = await screen.findAllByTestId("user-row");
    await userEvent.click(within(rows[0]).getByRole("button", { name: "Disable account" }));
    expect(await within(rows[0]).findByRole("button", { name: "Enable account" })).toBeInTheDocument();
    expect(within(rows[0]).getByText("Disabled")).toBeInTheDocument();
    await userEvent.click(within(rows[1]).getByRole("button", { name: "Disable account" }));
    await waitFor(() => expect(within(rows[1]).getByRole("alert")).toBeInTheDocument());
  });
});
