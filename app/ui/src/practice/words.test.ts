import { describe, expect, it } from "vitest";
import { splitWords } from "./Practice";

describe("filler word lists", () => {
  it("splits on ASCII and full-width commas and on 、", () => {
    expect(splitWords("嗯，啊、那个, 就是")).toEqual(["嗯", "啊", "那个", "就是"]);
    expect(splitWords("um, you know,, ")).toEqual(["um", "you know"]);
    expect(splitWords("")).toEqual([]);
  });
});
