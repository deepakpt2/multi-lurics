import { describe, expect, it, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { back, push } from "@/test/setup";
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

    expect(await calls.find((c) => c.method === "POST")!.json()).toMatchObject({
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

describe("composition form: devotional structure and video", () => {
  it("devotional songs get form, source and occasions; a pasted YouTube link is sent as typed (the server keeps the id)", async () => {
    const { calls } = mockFetch({ "GET /api/v1/entities/suggest": () => json(200, { items: [] }), "POST /api/v1/compositions": () => json(201, saved) });
    renderIntl(<CompositionForm />);
    expect(screen.queryByLabelText("Form")).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Title"), "Hanuman Chalisa");
    await userEvent.selectOptions(screen.getByLabelText("Song type"), "devotional");
    await userEvent.selectOptions(screen.getByLabelText("Form"), "chalisa");
    await userEvent.type(screen.getByLabelText("Source"), "Ramcharitmanas");
    await userEvent.type(screen.getByLabelText("Occasions"), "Tuesday, Hanuman Jayanti");
    await userEvent.type(screen.getByLabelText("YouTube video"), "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=9");
    expect(screen.queryByText(/does not look like a YouTube/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /save|create|add/i }));
    expect(await calls.find((c) => c.method === "POST")!.json()).toMatchObject({
      devotional_form: "chalisa",
      source_work: "Ramcharitmanas",
      occasions: ["Tuesday", "Hanuman Jayanti"],
      youtube: "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=9",
    });
  });

  it("says at once when a pasted link is not a YouTube video", async () => {
    mockFetch({ "GET /api/v1/entities/suggest": () => json(200, { items: [] }) });
    renderIntl(<CompositionForm />);
    await userEvent.type(screen.getByLabelText("YouTube video"), "https://vimeo.com/123");
    expect(screen.getByText(/does not look like a YouTube video link/)).toBeInTheDocument();
  });
});

describe("composition form: add missing details", () => {
  const existing = { ...saved, id: "c1", category: "devotional", raga: "Mohanam", religion: "hindu", deity: null, devotional_form: null, source_work: null, occasions: [], youtube_id: null, tags: ["bhajan"], year: null, iswc: null, created_by: "u9" };

  it("shows only what is empty, and sends only that", async () => {
    const { calls } = mockFetch({ "GET /api/v1/entities/suggest": () => json(200, { items: [] }), "PATCH /api/v1/compositions/c1": () => json(200, { ...existing, year: 1975 }) });
    renderIntl(<CompositionForm initial={existing as never} fillOnly />);
    // Filled in: not offered, so it cannot be changed from here.
    for (const name of ["Title", "Song type", "Raga", "Religion", "Tags"]) expect(screen.queryByLabelText(name)).not.toBeInTheDocument();
    for (const name of ["Year", "ISWC", "Deity", "Form", "Source", "Occasions", "YouTube video"]) expect(screen.getByLabelText(name)).toBeInTheDocument();
    expect(screen.getByText(/Anything already filled in stays as it is/)).toBeInTheDocument();

    await userEvent.type(screen.getByLabelText("Year"), "1975");
    await userEvent.type(screen.getByLabelText("Deity"), "Hanuman");
    await userEvent.click(screen.getByRole("button", { name: /save|create|add/i }));
    const sent = (await calls.find((c) => c.method === "PATCH")!.json()) as Record<string, unknown>;
    expect(sent).toMatchObject({ year: 1975, deity: "Hanuman", expected_version: 1 });
    for (const k of ["title", "category", "raga", "religion", "tags"]) expect(sent).not.toHaveProperty(k);
    // Back to the page the person came from, not to the catalogue tools.
    await vi.waitFor(() => expect(back).toHaveBeenCalled());
    expect(push).not.toHaveBeenCalled();
  });

  it("has nothing to offer when nothing is missing", async () => {
    const full = { ...existing, year: 1975, iswc: "T0345246801", deity: "Hanuman", devotional_form: "chalisa", source_work: "x", occasions: ["a"], youtube_id: "dQw4w9WgXcQ", credits: [{ artist_id: "a1", artist_name: "A", role: "composer" }] };
    mockFetch({ "GET /api/v1/entities/suggest": () => json(200, { items: [] }) });
    renderIntl(<CompositionForm initial={full as never} fillOnly />);
    expect(screen.getByText(/Nothing is missing here/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /save|create|add/i })).toBeDisabled();
  });
});
