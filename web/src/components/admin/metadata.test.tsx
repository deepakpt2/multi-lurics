import { describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, renderIntl } from "@/test/utils";
import { MetadataAdmin } from "./metadata";

const user = (roles: string[]) => ({ id: "u1", email: "u@example.com", display_name: "U", roles, email_verified: true });

describe("metadata values (admin)", () => {
  it("is for admins only", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, user(["moderator"])) });
    renderIntl(<MetadataAdmin />);
    expect(await screen.findByText("Only admins can create metadata values.")).toBeInTheDocument();
  });

  it("creates values in bulk, says which already existed, and keeps failures to fix", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/me": () => json(200, user(["admin"])),
      "POST /api/v1/admin/metadata": () =>
        json(200, { results: [{ name: "Mohanam", id: "1", created: true, problem: null }, { name: "kalyani", id: "2", created: false, problem: null }, { name: "x".repeat(5), id: null, created: false, problem: "too_long" }] }),
    });
    renderIntl(<MetadataAdmin />);
    await userEvent.click(await screen.findByRole("radio", { name: "Ragas" }));
    expect(screen.queryByLabelText("Type")).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Names, one per line"), "Mohanam{Enter}kalyani{Enter}xxxxx");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByText(/1 created, 1 already existed, 1 could not be made/)).toBeInTheDocument();
    expect(await calls.find((c) => c.method === "POST")!.json()).toEqual({ kind: "raga", names: ["Mohanam", "kalyani", "xxxxx"], sub_kind: null, year: null });
    expect(screen.getByLabelText("Names, one per line")).toHaveValue("xxxxx");
  });

  it("artists and albums have a type, albums a year", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, user(["admin"])) });
    renderIntl(<MetadataAdmin />);
    expect(await screen.findByLabelText("Type")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("radio", { name: "Albums" }));
    expect(screen.getByLabelText("Release year (all albums below)")).toBeInTheDocument();
  });
});
