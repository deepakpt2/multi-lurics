import { useState } from "react";
import { describe, expect, it } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, renderIntl } from "@/test/utils";
import { LanguagePicker } from "./language-picker";

const sanskrit = { id: "l1", iso639_3: "san", iso639_1: "sa", name_en: "Sanskrit", endonym: "संस्कृतम्", is_rtl: false, default_script: "Deva", is_extension: false };

function Harness({ start = "" }: { start?: string }) {
  const [code, setCode] = useState(start);
  return (
    <>
      <label htmlFor="lp">Language</label>
      <LanguagePicker id="lp" value={code} onChange={(c) => setCode(c)} />
      <output data-testid="value">{code}</output>
    </>
  );
}

describe("language picker", () => {
  it("typing 'sans' suggests Sanskrit, and choosing it sets the language", async () => {
    const { calls } = mockFetch({ "GET /api/v1/languages": () => json(200, { items: [sanskrit], next_cursor: null }) });
    renderIntl(<Harness />);
    await userEvent.type(screen.getByLabelText("Language"), "sans");
    await userEvent.click(await screen.findByRole("option", { name: /Sanskrit/ }));
    expect(screen.getByTestId("value")).toHaveTextContent("san");
    expect(screen.getByLabelText("Language")).toHaveValue("संस्कृतम् (Sanskrit)");
    expect(calls.some((c) => new URL(c.url).searchParams.get("q") === "sans")).toBe(true);
  });

  it("the first suggestion can be chosen with the keyboard", async () => {
    mockFetch({ "GET /api/v1/languages": () => json(200, { items: [sanskrit], next_cursor: null }) });
    renderIntl(<Harness />);
    await userEvent.type(screen.getByLabelText("Language"), "sa");
    await screen.findByRole("option", { name: /Sanskrit/ });
    await userEvent.keyboard("{Enter}");
    expect(screen.getByTestId("value")).toHaveTextContent("san");
  });

  it("text that was not chosen from the suggestions is not accepted", async () => {
    mockFetch({ "GET /api/v1/languages": () => json(200, { items: [], next_cursor: null }) });
    renderIntl(<Harness />);
    await userEvent.type(screen.getByLabelText("Language"), "klingon");
    expect(await screen.findByText(/No language matches/)).toBeInTheDocument();
    await userEvent.tab();
    expect(screen.getByLabelText("Language")).toHaveValue("");
    expect(screen.getByTestId("value")).toHaveTextContent("");
  });

  it("editing a chosen language withdraws the choice until another is picked", async () => {
    mockFetch({ "GET /api/v1/languages": () => json(200, { items: [sanskrit], next_cursor: null }) });
    renderIntl(<Harness />);
    await userEvent.type(screen.getByLabelText("Language"), "sans");
    await userEvent.click(await screen.findByRole("option", { name: /Sanskrit/ }));
    await userEvent.type(screen.getByLabelText("Language"), "x");
    expect(screen.getByTestId("value")).toHaveTextContent("");
  });

  it("shows a language given from outside by its name", async () => {
    mockFetch({ "GET /api/v1/languages": () => json(200, { items: [sanskrit], next_cursor: null }) });
    renderIntl(<Harness start="san" />);
    await waitFor(() => expect(screen.getByLabelText("Language")).toHaveValue("संस्कृतम् (Sanskrit)"));
  });
});
