import { cp, lstat, mkdir } from "node:fs/promises";
import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import type { Home } from "./home.ts";
import { houseOf } from "./layout.ts";
import { STARTERS } from "./library.ts";
import { named, RecordDoc } from "./record.ts";
import { canon, within } from "./sandbox/policy.ts";

/** A shipped starter copied into an agent's house at the chief's word (H25). */
export async function giveSkill(
    home: Home,
    hamlet: string,
    to: string,
    skill: string,
    context: Context,
): Promise<string> {
    if (!/^[a-zA-Z0-9_-]+$/.test(skill))
        throw new Error("a skill is named in letters, digits and hyphens");
    const record = await home.harness.snapshot(RecordDoc, hamlet, context);
    const agent = record === undefined ? undefined : named(record, to);
    if (agent === undefined) throw new Error(`no agent named \`${to}\``);
    const house = houseOf(home.layout, { hamlet, number: agent.number });
    const dest = join(house, "skills", skill);
    if (await lstat(dest).catch(() => undefined))
        throw new Error(
            `\`${agent.name}\` already has a skill named \`${skill}\``,
        );
    if (!within(canon(dest), canon(house)))
        throw new Error("a skill must stay in the agent's house");
    const source = join(STARTERS, skill);
    if (
        !(
            await lstat(join(source, "SKILL.md")).catch(() => undefined)
        )?.isFile()
    )
        throw new Error(
            `no starter skill named \`${skill}\`. Say whose it is: /give ${agent.name} ${skill} from <agent>`,
        );
    try {
        await mkdir(dest);
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "EEXIST")
            throw new Error(
                `\`${agent.name}\` already has a skill named \`${skill}\``,
            );
        throw error;
    }
    await cp(source, dest, {
        recursive: true,
        force: false,
        errorOnExist: true,
    });
    await home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, hamlet, hamlet)).decisions.push({
            kind: "skill_given",
            at: home.now(),
            by: home.chief,
            to: agent.number,
            skill,
            from: null,
        });
    }, context);
    return `Gave \`${to}\` the skill \`${skill}\`.`;
}
