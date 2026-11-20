import { describe, expect, it } from "vitest";
import { embedUrl, watchUrl, youtubeId } from "./youtube";

describe("youtubeId", () => {
  it.each([
    "dQw4w9WgXcQ",
    "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    "http://youtube.com/watch?feature=share&v=dQw4w9WgXcQ&t=42",
    "youtube.com/watch?v=dQw4w9WgXcQ",
    "https://m.youtube.com/watch?v=dQw4w9WgXcQ#t=1",
    "https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=x",
    "https://youtu.be/dQw4w9WgXcQ?si=abc",
    "https://www.youtube.com/embed/dQw4w9WgXcQ",
    "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?rel=0",
    "https://www.youtube.com/shorts/dQw4w9WgXcQ",
    "https://www.youtube.com/live/dQw4w9WgXcQ?feature=share",
    "  https://youtu.be/dQw4w9WgXcQ  ",
  ])("reads the id from %s", (link) => expect(youtubeId(link)).toBe("dQw4w9WgXcQ"));

  it.each(["", "  ", "https://vimeo.com/123456789", "https://youtube.com/watch?v=short", "https://evil.example/watch?v=dQw4w9WgXcQ", "not a link"])(
    "refuses %j",
    (bad) => expect(youtubeId(bad)).toBeNull(),
  );

  it("builds the player and watch addresses from the id alone", () => {
    expect(embedUrl("dQw4w9WgXcQ")).toBe("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?rel=0");
    expect(watchUrl("dQw4w9WgXcQ")).toBe("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
  });
});
