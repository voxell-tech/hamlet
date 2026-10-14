import { randomUUID } from "node:crypto";
import {
    readdir,
    readFile,
    rename,
    symlink,
    writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import { ChatDoc } from "../src/chats.ts";
import { HOME } from "../src/hamlet.ts";
import type { Home } from "../src/home.ts";
import { houseOf } from "../src/layout.ts";
import { RecordDoc } from "../src/record.ts";
import { say } from "../src/say.ts";
import { context, fauxModels, MAYOR, scratch, testHome } from "./support.ts";

it("gives the mayor a shipped starter, keeps every file and records the chief's gift without a turn", async () => {
    const { models, faux } = fauxModels();
    const dir = scratch();
    let home = await testHome(dir, { models });
    const skill = join(houseOf(home.layout, MAYOR), "skills/web-search");
    const shipped = fileURLToPath(
        new URL("../assets/starters/web-search/", import.meta.url),
    );
    expect(await typed(home, "/give mayor web-search")).toMatchObject({
        kind: "notice",
        text: "Gave `mayor` the skill `web-search`.",
    });
    expect(await readFile(join(skill, "SKILL.md"), "utf8")).toBe(
        await readFile(join(shipped, "SKILL.md"), "utf8"),
    );
    expect(await readFile(join(skill, "scripts/search.py"), "utf8")).toBe(
        await readFile(join(shipped, "scripts/search.py"), "utf8"),
    );
    expect(
        (await home.harness.snapshot(RecordDoc, HOME, context))?.decisions,
    ).toContainEqual({
        kind: "skill_given",
        at: expect.any(Number),
        by: "chief",
        to: 0,
        skill: "web-search",
        from: null,
    });
    expect(await typed(home, "/give mayor web-search")).toMatchObject({
        kind: "error",
        text: "`founder` already has a skill named `web-search`",
    });
    expect(await typed(home, "/give mayor ../maps")).toMatchObject({
        kind: "error",
        text: "a skill is named in letters, digits and hyphens",
    });
    expect(await typed(home, "/give mayor nothing")).toMatchObject({
        kind: "error",
        text: "no starter skill named `nothing`. Say whose it is: /give founder nothing from <agent>",
    });
    expect(await typed(home, "/give nobody web-search")).toMatchObject({
        kind: "error",
        text: "no agent named `nobody`",
    });
    expect(
        await typed(home, "/give mayor web-search from scout"),
    ).toMatchObject({ kind: "error", text: "usage: /give <agent> <starter>" });
    expect(
        (await home.harness.snapshot(ChatDoc, "home:0", context))?.chats,
    ).toEqual({});
    await home.close(context);
    home = await testHome(dir, { models });
    expect(await typed(home, "/give founder web-search")).toMatchObject({
        kind: "error",
        text: "`founder` already has a skill named `web-search`",
    });
    expect(faux.state.callCount).toBe(0);
});

it("refuses a skill whose destination is a link and leaves what is there intact", async () => {
    const { models } = fauxModels();
    const home = await testHome(scratch(), { models });
    const outside = scratch();
    await writeFile(join(outside, "SKILL.md"), "Keep me.");
    const dest = join(houseOf(home.layout, MAYOR), "skills/web-search");
    await symlink(outside, dest);
    expect(await typed(home, "/give mayor web-search")).toMatchObject({
        kind: "error",
        text: "`founder` already has a skill named `web-search`",
    });
    expect(await readFile(join(outside, "SKILL.md"), "utf8")).toBe("Keep me.");
});

function typed(home: Home, text: string) {
    return say(
        home,
        {
            type: "say",
            id: randomUUID(),
            to: { hamlet: HOME, agent: "mayor" },
            text,
        },
        context,
    );
}

it("refuses a linked skills directory rather than writing outside the house", async () => {
    const { models } = fauxModels();
    const home = await testHome(scratch(), { models });
    const outside = scratch();
    const skills = join(houseOf(home.layout, MAYOR), "skills");
    await rename(skills, `${skills}-kept`);
    await symlink(outside, skills);
    expect(await typed(home, "/give mayor web-search")).toMatchObject({
        kind: "error",
        text: "a skill must stay in the agent's house",
    });
    expect(await readdir(outside)).toEqual([]);
});
