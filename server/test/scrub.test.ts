import { describe, expect, it } from "vitest";
import { Scrubber } from "../src/scrub.ts";

/** The text `chunks` give through a scrubber of `secrets`, chunk by chunk, then the rest it held back. */
function through(secrets: string[], chunks: string[]): string[] {
    const scrubber = new Scrubber(secrets);
    return [...chunks.map((chunk) => scrubber.push(chunk)), scrubber.flush()];
}

describe("the scrubber (C32; log.rs:399)", () => {
    it("replaces every secret it knows, also across chunks", () => {
        expect(
            through(["sk-abc", "tok-123"], ["key sk-abc and tok-123 end"]),
        ).toEqual(["key [REDACTED] and [REDACTED] end", ""]);
        expect(through(["sk-abc"], ["key sk-", "abc end"])).toEqual([
            "key ",
            "[REDACTED] end",
            "",
        ]);
        expect(
            through(["sk-abc"], ["s", "k", "-", "a", "b", "c!"]).join(""),
        ).toBe("[REDACTED]!");
    });

    it("holds back no more than the longest secret's length minus one, and lets it out at the end", () => {
        const secrets = ["abcdef", "xyz"];
        const scrubber = new Scrubber(secrets);
        const out = scrubber.push("0123abcde");

        expect(out).toBe("0123");
        expect(scrubber.push("")).toBe("");
        expect(scrubber.flush()).toBe("abcde");
        expect(through(secrets, ["text xy"])).toEqual(["text ", "xy"]);
        expect(through(secrets, ["plain"])).toEqual(["plain", ""]);
    });

    it("lets everything through when it knows no secret", () => {
        expect(through([], ["a", "b"])).toEqual(["a", "b", ""]);
    });
});
