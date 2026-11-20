import { describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { ContributeButton } from "./contribute";

const me = { id: "u1", email: "u@example.com", display_name: "U", roles: [], email_verified: true };

describe("contribute button", () => {
  it("asks a signed-out visitor to sign in", async () => {
    mockFetch({ "GET /api/v1/me": () => problem(401, "unauthenticated") });
    renderIntl(<ContributeButton target={{ kind: "song", id: "c1", recordingId: "r1", sourceId: "l1" }} />);
    expect(await screen.findByRole("link", { name: "Sign in to contribute" })).toHaveAttribute("href", "/login");
  });

  it("offers a signed-in member translations, transliterations, other-script lyrics and missing details of a song", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me) });
    renderIntl(<ContributeButton target={{ kind: "song", id: "c1", recordingId: "r1", sourceId: "l1" }} />);
    await userEvent.click(await screen.findByRole("button", { name: "Contribute" }));
    expect(screen.getByRole("menuitem", { name: "Add a translation" })).toHaveAttribute("href", "/recordings/r1/lyrics/new?type=translation&source=l1");
    expect(screen.getByRole("menuitem", { name: "Add a transliteration" })).toHaveAttribute("href", "/recordings/r1/lyrics/new?type=transliteration&source=l1");
    expect(screen.getByRole("menuitem", { name: "Add lyrics in another language or script" })).toHaveAttribute("href", "/recordings/r1/lyrics/new?type=original");
    expect(screen.getByRole("menuitem", { name: "Add missing details" })).toHaveAttribute("href", "/compositions/c1/complete");
  });

  it("an artist or album page offers its missing details and points to the photo button", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me) });
    renderIntl(<ContributeButton target={{ kind: "artist", id: "a1" }} />);
    await userEvent.click(await screen.findByRole("button", { name: "Contribute" }));
    expect(screen.getByRole("menuitem", { name: "Add missing details" })).toHaveAttribute("href", "/artists/a1/complete");
    expect(screen.queryByRole("menuitem", { name: "Add a translation" })).not.toBeInTheDocument();
    expect(screen.getByText(/camera on the picture/)).toBeInTheDocument();
  });
});
