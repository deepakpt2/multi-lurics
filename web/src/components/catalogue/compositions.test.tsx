import { describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { push } from "@/test/setup";
import { CompositionForm } from "./compositions";

const saved = { id: "c9", title: "Hanuman Chalisa", version: 1, credits: [], recordings: [], recording_count: 0 };

describe("composition form: Indian song metadata", () => {
  it("shows religion and deity only for devotional songs and sends the metadata", async () => {
    const { calls } = mockFetch({ "GET /api/v1/entities/suggest": () => json(200, { items: [] }), "POST /api/v1/compositions": () => json(201, saved) });
    renderIntl(<CompositionForm />);
    expect(screen.queryByLabelText("Religion")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Deity")).not.toBeInTheDocument();

    await userEvent.type(screen.getByLabelText("Title"), "Hanuman Chalisa");
    await userEvent.selectOptions(screen.getByLabelText("Song type"), "devotional");
    await userEvent.type(screen.getByLabelText("Raga"), "Mohanam");
    await userEvent.selectOptions(screen.getByLabelText("Religion"), "hindu");
    await userEvent.type(screen.getByLabelText("Deity"), "Hanuman");
    await userEvent.type(screen.getByLabelText("Tags"), "bhajan, morning ,");
    await userEvent.click(screen.getByRole("button", { name: /save|create|add/i }));

    expect(await calls.at(-1)!.json()).toMatchObject({
      title: "Hanuman Chalisa",
      category: "devotional",
      raga: "Mohanam",
      religion: "hindu",
      deity: "Hanuman",
      tags: ["bhajan", "morning"],
    });
    expect(push).toHaveBeenCalledWith("/compositions/c9");
  });

  it("never sends religion or deity for a mainstream song, and shows server errors on the field", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/entities/suggest": () => json(200, { items: [] }),
      "POST /api/v1/compositions": () => problem(422, "validation_failed", { errors: [{ field: "tags", code: "too_many" }] }),
    });
    renderIntl(<CompositionForm />);
    await userEvent.type(screen.getByLabelText("Title"), "Film song");
    await userEvent.click(screen.getByRole("button", { name: /save|create|add/i }));
    expect(await screen.findByText("There are too many entries.")).toBeInTheDocument();
    expect(await calls[0].json()).toMatchObject({ category: "mainstream", religion: null, deity: null, tags: [] });
  });

  it("tells the user which existing raga their text will link to", async () => {
    mockFetch({
      "GET /api/v1/entities/suggest": () =>
        json(200, { items: [{ id: "r1", type: "raga", display: { text: "Mohanam", dir: "ltr", resolution: "primary" }, localized_names: [], matched_via: "name", related: [], score: 1 }] }),
    });
    renderIntl(<CompositionForm />);
    await userEvent.type(screen.getByLabelText("Raga"), "mohanam");
    expect(await screen.findByText(/Matches existing:/)).toBeInTheDocument();
    expect(screen.getByText("Mohanam")).toBeInTheDocument();
  });
});
