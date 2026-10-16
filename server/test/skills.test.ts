import { mkdir, rename, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { parseSkill } from "../src/house.ts";
import { swappingDirectory } from "./race-support.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

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

it("reads a whole house shelf without paying a process startup for each paper", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const kept = home.houses.of({ hamlet: "home", number: 0 });
    const house = kept.path;
    for (let n = 0; n < 16; n++) {
        const dir = join(house, "skills", `skill-${n}`);
        await mkdir(dir, { recursive: true });
        await writeFile(
            join(dir, "SKILL.md"),
            `---\nname: skill-${n}\ndescription: Paper ${n}\n---\n`,
        );
    }
    const other = await testHome(scratch(), { models: fauxModels().models });
    const smallHouse = other.houses.of({ hamlet: "home", number: 0 });
    const small = smallHouse.path;
    await mkdir(join(small, "skills", "one"), { recursive: true });
    await writeFile(
        join(small, "skills", "one", "SKILL.md"),
        "---\nname: one\n---\n",
    );
    await smallHouse.skills(context);
    await kept.skills(context);
    const measure = async (reader: typeof kept, count: number) => {
        const start = performance.now();
        for (let n = 0; n < 3; n++)
            expect(await reader.skills(context)).toHaveLength(count);
        return performance.now() - start;
    };
    const one = await measure(smallHouse, 1);
    const many = await measure(kept, 16);
    expect(many).toBeLessThan(one * 4 + 250);
});

it("never lists outside skill names while the shelf is swapped for a link", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const kept = home.houses.of({ hamlet: "home", number: 0 });
    const house = kept.path;
    const outside = scratch();
    const dir = join(house, "skills");
    await mkdir(join(dir, "inside"), { recursive: true });
    await writeFile(
        join(dir, "inside", "SKILL.md"),
        "---\nname: inside\n---\n",
    );
    await mkdir(join(outside, "outside-secret-name"));
    await writeFile(
        join(outside, "outside-secret-name", "SKILL.md"),
        "---\nname: outside-secret-name\ndescription: outside-secret\n---\n",
    );
    await swappingDirectory(dir, outside);
    for (let n = 0; n < 1000; n++) {
        home.houses.invalidate(house);
        expect(await kept.skillNames(context)).not.toContain(
            "outside-secret-name",
        );
        if (n % 20 === 0)
            expect(JSON.stringify(await kept.skills(context))).not.toContain(
                "outside-secret",
            );
    }
}, 20_000);

it("refreshes changed and replaced papers on the next request", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const kept = home.houses.of({ hamlet: "home", number: 0 });
    const house = kept.path;
    const dir = join(house, "skills", "one");
    const path = join(dir, "SKILL.md");
    await mkdir(dir, { recursive: true });
    await writeFile(path, "---\nname: one\ndescription: First\n---\n");
    expect(await kept.skills(context)).toMatchObject([
        { description: "First" },
    ]);
    await writeFile(path, "---\nname: one\ndescription: Later\n---\n");
    expect(await kept.skills(context)).toMatchObject([
        { description: "Later" },
    ]);
    await writeFile(
        `${path}-next`,
        "---\nname: one\ndescription: New inode\n---\n",
    );
    await rename(`${path}-next`, path);
    expect(await kept.skills(context)).toMatchObject([
        { description: "New inode" },
    ]);
});
