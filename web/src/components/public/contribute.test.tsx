import { describe, expect, it } from "vitest";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { ContributeButton } from "./contribute";

const me = { id: "u1", email: "u@example.com", display_name: "U", roles: [], email_verified: true };
const page = (items: unknown[]) => ({ items, next_cursor: null });
const refs = {
  "GET /api/v1/languages": () => json(200, page([{ id: "l1", iso639_3: "eng", iso639_1: "en", name_en: "English", endonym: "English", is_rtl: false, default_script: "Latn" }])),
  "GET /api/v1/scripts": () => json(200, page([{ id: "s1", iso15924: "Latn", name_en: "Latin", is_rtl: false }])),
};

describe("contribute button", () => {
  it("asks a signed-out visitor to sign in", async () => {
    mockFetch({ "GET /api/v1/me": () => problem(401, "unauthenticated") });
    renderIntl(<ContributeButton target={{ kind: "song", id: "c1" }} />);
    expect(await screen.findByRole("link", { name: "Sign in to contribute" })).toHaveAttribute("href", "/login");
  });

  it("a song's menu adds lyrics in a window on the same page, with no mention of recordings", async () => {
    const { calls } = mockFetch({ "GET /api/v1/me": () => json(200, me), ...refs, "POST /api/v1/compositions/c1/lyrics": () => json(201, { status: "pending" }) });
    renderIntl(<ContributeButton target={{ kind: "song", id: "c1", title: "Kannum" }} />);
    await userEvent.click(await screen.findByRole("button", { name: "Contribute" }));
    expect(screen.getByRole("menuitem", { name: "Add missing details" })).toHaveAttribute("href", "/compositions/c1/complete");
    expect(screen.queryByText(/recording/i)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("menuitem", { name: "Add a transliteration" }));

    const dialog = await screen.findByRole("dialog", { name: "Add lyrics: Kannum" });
    expect(within(dialog).getByRole("radio", { name: "Transliteration" })).toBeChecked();
    await userEvent.selectOptions(await within(dialog).findByLabelText("Language"), "eng");
    await userEvent.type(within(dialog).getByLabelText("Lyrics"), "kannum kannum");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save lyrics" }));
    expect(await screen.findByText(/once a moderator has checked them/)).toBeInTheDocument();
    expect(await calls.find((c) => c.method === "POST")!.json()).toMatchObject({ type: "transliteration", language: "eng", script: "Latn", body: "kannum kannum" });
    await userEvent.click(screen.getByRole("button", { name: "OK" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
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
