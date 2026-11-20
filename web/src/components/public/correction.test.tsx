import { describe, expect, it } from "vitest";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { CorrectionButton } from "./correction";

const me = (roles: string[]) => ({ id: "u1", email: "u@example.com", display_name: "U", roles, email_verified: true });
const show = () => renderIntl(<CorrectionButton id="c1" body="old line" language="ml" dir="ltr" name="Malayalam" />);

describe("correction button", () => {
  it("sends a signed-out reader to sign in", async () => {
    mockFetch({ "GET /api/v1/me": () => problem(401, "unauthenticated") });
    show();
    expect(await screen.findByRole("link", { name: "Sign in to suggest a correction" })).toHaveAttribute("href", "/login");
  });

  it("lets a moderator edit the lyric directly", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me(["moderator"])) });
    show();
    expect(await screen.findByRole("link", { name: "Edit Malayalam" })).toHaveAttribute("href", "/contributions/c1/edit");
  });

  it("lets a member send a corrected text for review", async () => {
    const { calls } = mockFetch({ "GET /api/v1/me": () => json(200, me([])), "POST /api/v1/contributions/c1/corrections": () => json(201, { id: "k1" }) });
    show();
    await userEvent.click(await screen.findByRole("button", { name: "Suggest a correction to Malayalam" }));
    const dialog = screen.getByRole("dialog", { name: "Suggest a correction" });
    const send = within(dialog).getByRole("button", { name: "Send correction" });
    expect(send).toBeDisabled(); // nothing changed yet
    const text = within(dialog).getByLabelText("Lyrics");
    expect(text).toHaveValue("old line");
    await userEvent.clear(text);
    await userEvent.type(text, "new line");
    await userEvent.type(within(dialog).getByLabelText(/What was wrong/), "typo");
    await userEvent.click(send);
    expect(await screen.findByText(/A moderator will review your correction/)).toBeInTheDocument();
    expect(await calls.find((c) => c.method === "POST")!.json()).toEqual({ body: "new line", note: "typo" });
  });
});
