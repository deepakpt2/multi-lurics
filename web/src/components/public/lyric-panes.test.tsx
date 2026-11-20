import { beforeEach, describe, expect, it } from "vitest";
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, renderIntl } from "@/test/utils";
import { READING_KEY, type Lyric } from "@/lib/lyric-panes";
import { LyricPanes } from "./lyric-panes";

const lyric = (id: string, type: string, language: string, body: string, script = "Mlym"): Lyric =>
  ({ id, recording_id: "r1", type, language, script, dir: "ltr", is_primary: id === "a", body, revised_at: "2026-01-01T00:00:00Z" }) as Lyric;
const lyrics = [
  lyric("a", "original", "ml", "കാറ്റിൻ മൊഴി"),
  lyric("b", "translation", "en", "Words of the wind", "Latn"),
  lyric("c", "translation", "hi", "हवा की बोली", "Deva"),
  lyric("d", "transliteration", "ml", "kaattin mozhi", "Latn"),
];

const show = (props: Partial<React.ComponentProps<typeof LyricPanes>> = {}) =>
  renderIntl(<LyricPanes lyrics={lyrics} songPath="/song/kaatrin-mozhi" pageLocale="en" songId="s1" title="Kaatrin Mozhi" {...props} />);

beforeEach(() => {
  window.localStorage.clear();
  mockFetch({
    "GET /api/v1/languages": () => json(200, { items: [], next_cursor: null }),
    "GET /api/v1/scripts": () => json(200, { items: [], next_cursor: null }),
  });
});

describe("lyric columns", () => {
  it("opens with the song's main lyric and a + to add another column", () => {
    show();
    expect(screen.getAllByRole("article")).toHaveLength(1);
    expect(screen.getByText("കാറ്റിൻ മൊഴി")).toHaveAttribute("lang", "ml");
    expect(screen.getByRole("button", { name: "Add a column" })).toBeInTheDocument();
    // One column cannot be closed: there would be nothing left to read.
    expect(screen.queryByRole("button", { name: /Close column/ })).not.toBeInTheDocument();
  });

  it("adds a column with its own type and the languages that exist for that type", async () => {
    show();
    await userEvent.click(screen.getByRole("button", { name: "Add a column" }));
    const second = screen.getByRole("article", { name: "Column 2" });
    // It starts on the first type that still has something to show: translation.
    const type = within(second).getByLabelText("Type");
    expect(type).toHaveValue("translation");
    const language = within(second).getByLabelText("Language");
    expect(within(language).getAllByRole("option").map((o) => o.textContent)).toEqual(["Choose language…", "English", "Hindi"]);
    expect(within(second).getByText("Choose a language to read it here.")).toBeInTheDocument();

    await userEvent.selectOptions(language, "c");
    expect(within(second).getByText("हवा की बोली")).toHaveAttribute("lang", "ml".replace("ml", "hi"));
    // The first column is untouched.
    expect(screen.getByText("കാറ്റിൻ മൊഴി")).toBeInTheDocument();

    // A transliteration says which script it is written in.
    await userEvent.selectOptions(type, "transliteration");
    expect(within(language).getAllByRole("option").map((o) => o.textContent)).toEqual(["Choose language…", "Malayalam · Latin"]);
    expect(within(second).queryByText("हवा की बोली")).not.toBeInTheDocument();
  });

  it("any number of columns, each closable and each with its own full-screen link", async () => {
    show();
    await userEvent.click(screen.getByRole("button", { name: "Add a column" }));
    await userEvent.selectOptions(within(screen.getByRole("article", { name: "Column 2" })).getByLabelText("Language"), "b");
    await userEvent.click(screen.getByRole("button", { name: "Add a column" }));
    await userEvent.selectOptions(within(screen.getByRole("article", { name: "Column 3" })).getByLabelText("Language"), "c");
    expect(screen.getAllByRole("article")).toHaveLength(3);
    expect(screen.getByRole("link", { name: "Open column 1 full screen" })).toHaveAttribute("href", "/song/kaatrin-mozhi/lyrics?l=a");
    expect(screen.getByRole("link", { name: "Open column 2 full screen" })).toHaveAttribute("href", "/song/kaatrin-mozhi/lyrics?l=b");
    expect(screen.getByRole("link", { name: "Open column 3 full screen" })).toHaveAttribute("href", "/song/kaatrin-mozhi/lyrics?l=c");
    expect(screen.getByText("Column 1 of 3", { exact: false })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Close column 2" }));
    expect(screen.getAllByRole("article")).toHaveLength(2);
    expect(screen.queryByText("Words of the wind")).not.toBeInTheDocument();
  });

  it("a type with nothing yet offers to add one", async () => {
    show({ lyrics: [lyric("a", "original", "ml", "x")] });
    await userEvent.click(screen.getByRole("button", { name: "Add a column" }));
    const second = screen.getByRole("article", { name: "Column 2" });
    expect(within(second).getByText(/There is no translation for this song yet/)).toBeInTheDocument();
    // "Add one" opens the lyrics window for that type, on this page.
    await userEvent.selectOptions(within(second).getByLabelText("Type"), "transliteration");
    await userEvent.click(within(second).getByRole("button", { name: "Add one" }));
    const dialog = await screen.findByRole("dialog", { name: "Add lyrics: Kaatrin Mozhi" });
    expect(within(dialog).getByRole("radio", { name: "Transliteration" })).toBeChecked();
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    // The original has a lyric to read, so no offer to add one.
    await userEvent.selectOptions(within(second).getByLabelText("Type"), "original");
    expect(within(second).queryByRole("button", { name: "Add one" })).not.toBeInTheDocument();
  });

  it("a full-screen view opens the columns its address names and keeps the address up to date", async () => {
    window.history.replaceState(null, "", "/en/song/kaatrin-mozhi/lyrics?l=d,b");
    show({ initial: ["d", "b"], fullScreen: true });
    expect(screen.getByText("kaattin mozhi")).toBeInTheDocument();
    expect(screen.getByText("Words of the wind")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /full screen/ })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Close column 1" }));
    expect(new URL(window.location.href).searchParams.get("l")).toBe("b");
    await userEvent.click(screen.getByRole("button", { name: "Add a column" }));
    const added = screen.getByRole("article", { name: "Column 2" });
    await userEvent.selectOptions(within(added).getByLabelText("Type"), "translation");
    await userEvent.selectOptions(within(added).getByLabelText("Language"), "c");
    expect(new URL(window.location.href).searchParams.get("l")).toBe("b,c");
  });
});

describe("reading mode", () => {
  it("shows the lyrics large over everything else, remembers size and colours, and closes with Escape", async () => {
    show();
    await userEvent.click(screen.getByRole("button", { name: "Reading mode" }));
    const dialog = screen.getByRole("dialog", { name: "Reading mode" });
    expect(within(dialog).getByText("കാറ്റിൻ മൊഴി")).toBeInTheDocument();
    expect(document.body).toHaveClass("reading-open");
    expect(dialog).toHaveAttribute("data-theme", "light");
    const size = () => dialog.style.getPropertyValue("--reading-size");
    const before = size();
    await userEvent.click(within(dialog).getByRole("button", { name: "Larger text" }));
    expect(size()).not.toBe(before);
    await userEvent.click(within(dialog).getByRole("radio", { name: "Sepia" }));
    expect(dialog).toHaveAttribute("data-theme", "sepia");
    expect(JSON.parse(window.localStorage.getItem(READING_KEY)!)).toEqual({ size: 3, theme: "sepia" });

    // Columns can still be added while reading.
    await userEvent.click(within(dialog).getByRole("button", { name: "Add a column" }));
    expect(within(dialog).getAllByRole("article")).toHaveLength(2);

    await act(async () => {
      await userEvent.keyboard("{Escape}");
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(document.body).not.toHaveClass("reading-open");
    // The columns are still there afterwards.
    expect(screen.getAllByRole("article")).toHaveLength(2);
  });

  it("comes back the way it was left", async () => {
    window.localStorage.setItem(READING_KEY, JSON.stringify({ size: 5, theme: "dark" }));
    show();
    await userEvent.click(screen.getByRole("button", { name: "Reading mode" }));
    expect(screen.getByRole("dialog")).toHaveAttribute("data-theme", "dark");
    expect(screen.getByRole("radio", { name: "Dark" })).toBeChecked();
  });
});
