import { describe, expect, it } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { json, mockFetch, problem, renderIntl } from "@/test/utils";
import { EntityImage } from "./entity-image";
import { ImageUpload } from "./image-upload";

const limits = { max_bytes: 1024 * 1024, min_side: 64, max_side: 4000, sizes: [96, 240, 640], types: ["image/png"] };
const me = { id: "u1", email: "a@example.com", display_name: "A", roles: [], email_verified: true };
const png = () => new File([new Uint8Array([137, 80, 78, 71])], "me.png", { type: "image/png" });

describe("entity image", () => {
  it("shows the real photo, sized for the screen, when there is one", () => {
    renderIntl(<EntityImage type="artist" id="a1" imageId="img-1" name="Yesudas" size={48} />);
    const img = document.querySelector("img")!;
    expect(img.getAttribute("src")).toBe("/api/v1/media/img-1/96");
    expect(img.getAttribute("srcset")).toBe("/api/v1/media/img-1/48 1x, /api/v1/media/img-1/96 2x");
    expect(screen.getByRole("button", { name: "Suggest a different photo of Yesudas" })).toBeInTheDocument();
  });

  it("shows a placeholder with a small button to add a real photo", async () => {
    mockFetch({ "GET /api/v1/me": () => json(200, me), "GET /api/v1/images/limits": () => json(200, limits) });
    renderIntl(<EntityImage type="deity" id="d1" imageId={null} name="Krishna" />);
    expect(screen.getByRole("img", { name: "No photo of Krishna yet" })).toBeInTheDocument();
    expect(document.querySelector("img")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Add a photo of Krishna" }));
    expect(await screen.findByLabelText("Choose a photo")).toBeInTheDocument();
  });

  it("asks a signed-out visitor to sign in instead of showing the form", async () => {
    mockFetch({ "GET /api/v1/me": () => problem(401, "unauthenticated") });
    renderIntl(<EntityImage type="album" id="al1" imageId={null} name="Kerala" />);
    await userEvent.click(screen.getByRole("button", { name: "Add a photo of Kerala" }));
    expect(await screen.findByRole("link", { name: "Sign in to add a photo" })).toBeInTheDocument();
  });

  it("offers no edit button where the entity is not known", () => {
    renderIntl(<EntityImage type="artist" imageId={null} name="Someone" />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});

describe("image upload", () => {
  it("sends the file, consent, credit and entity, and reports the outcome", async () => {
    const { calls } = mockFetch({
      "GET /api/v1/images/limits": () => json(200, limits),
      "POST /api/v1/images": () => json(201, { id: "i1", status: "pending", width: 200, height: 200 }),
    });
    let got: { status: string } | undefined;
    renderIntl(<ImageUpload entity={{ type: "artist", id: "a1" }} onUploaded={(i) => (got = i)} />);
    const send = screen.getByRole("button", { name: "Upload photo" });
    expect(send).toBeDisabled();
    await userEvent.upload(screen.getByLabelText("Choose a photo"), png());
    expect(send).toBeDisabled(); // consent is still missing
    await userEvent.click(screen.getByLabelText(/I took this photo/));
    await userEvent.type(screen.getByLabelText(/Credit or source/), "Own photo");
    await userEvent.click(send);
    await waitFor(() => expect(got?.status).toBe("pending"));
    const post = calls.find((c) => c.method === "POST")!;
    expect(post.headers.get("x-requested-with")).toBe("fetch");
    // (jsdom's FormData is not the browser's, so the multipart fields are checked in the API tests.)
  });

  it("refuses a file over the limit without sending it", async () => {
    const { calls } = mockFetch({ "GET /api/v1/images/limits": () => json(200, limits) });
    renderIntl(<ImageUpload onUploaded={() => undefined} />);
    await screen.findByText(/Up to 1 MB/);
    const big = new File([new Uint8Array(1024 * 1024 + 1)], "big.png", { type: "image/png" });
    await userEvent.upload(screen.getByLabelText("Choose a photo"), big);
    expect(await screen.findByText("This file is larger than 1 MB.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Upload photo" })).toBeDisabled();
    expect(calls.some((c) => c.method === "POST")).toBe(false);
  });

  it("shows the server's reason next to the file", async () => {
    mockFetch({
      "GET /api/v1/images/limits": () => json(200, limits),
      "POST /api/v1/images": () => problem(422, "validation_failed", { errors: [{ field: "file", code: "unsupported_type" }] }),
    });
    renderIntl(<ImageUpload onUploaded={() => undefined} />);
    await userEvent.upload(screen.getByLabelText("Choose a photo"), png());
    await userEvent.click(screen.getByLabelText(/I took this photo/));
    await userEvent.click(screen.getByRole("button", { name: "Upload photo" }));
    expect(await screen.findByText("Use a JPEG, PNG, WebP or GIF image.")).toBeInTheDocument();
  });
});
