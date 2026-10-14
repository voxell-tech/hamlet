import { describe, expect, it } from "vitest";
import { parseSkill } from "../src/skills.ts";

describe("a skill's front matter (skill.rs:16-46)", () => {
    it("names the skill, says what it does and how it runs", () => {
        expect(
            parseSkill(
                '---\nname: forecast\ndescription: "Get the forecast"\nusage: sh skills/forecast/scripts/get.sh\n---\nRun scripts/get.sh\n',
            ),
        ).toEqual({
            name: "forecast",
            description: "Get the forecast",
            usage: "sh skills/forecast/scripts/get.sh",
        });
    });

    it("is required, and must name the skill", () => {
        expect(parseSkill("no front matter")).toBeUndefined();
        expect(parseSkill("---\ndescription: Nameless\n---\n")).toBeUndefined();
    });

    it("takes the first line of each field, and runs to the end of a file that never closes it", () => {
        expect(
            parseSkill(
                "---\r\n name : tides \r\nname: second\r\nusage:\r\ndescription: When the sea comes in",
            ),
        ).toEqual({
            name: "tides",
            description: "When the sea comes in",
            usage: undefined,
        });
    });

    it("ends at the first line that opens with ---", () => {
        expect(
            parseSkill("---\nname: a\n---\ndescription: in the body\n"),
        ).toEqual({ name: "a", description: "", usage: undefined });
    });
});
