import { randomUUID } from "node:crypto";
import { lstat, mkdir, mkdtemp, open, readdir, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { submitLine } from "./chats.ts";
import {
    checkShelf,
    checkSkill,
    deliver,
    manifestSize,
    stageSkill,
} from "./gift-files.ts";
import type { Home } from "./home.ts";
import { houseOf } from "./home.ts";
import { STARTERS } from "./library.ts";
import { safeWords } from "./plainly.ts";
import {
    type Address,
    type Give,
    type GivePart,
    MAX_GIFT_BYTES,
    type Reply,
} from "./protocol.ts";
import { type ChiefResult, chiefAccess, chiefRefusalText } from "./realm.ts";
import type { AgentRef } from "./record.ts";
import { named, RecordDoc } from "./record.ts";
import { directoryRoot } from "./sandbox/policy.ts";
import { refreshSkills } from "./skills.ts";

const IDLE_MS = 60_000;
const STAGE_REFUSED = "The gift could not be staged. Nothing was handed over.";

async function staged(home: Home): Promise<string> {
    try {
        const root = home.layout.gifts;
        directoryRoot(root);
        await mkdir(root, { recursive: true, mode: 0o700 });
        directoryRoot(root);
        return await mkdtemp(join(root, "gift-"));
    } catch {
        throw new Error(STAGE_REFUSED);
    }
}

async function resolved(
    home: Home,
    to: Address,
    context: Context,
): Promise<ChiefResult<{ ref: AgentRef; name: string }>> {
    const access = await chiefAccess(home, to.hamlet, "give", context);
    if (!access.ok) return access;
    const record = await home.harness.snapshot(RecordDoc, to.hamlet, context);
    const agent = record === undefined ? undefined : named(record, to.agent);
    if (agent === undefined) throw new Error(`no agent named \`${to.agent}\``);
    return {
        ok: true,
        value: {
            ref: { hamlet: to.hamlet, number: agent.number },
            name: agent.name,
        },
    };
}

/** Abandoned gifts from an earlier server process. */
export async function clearStagedGifts(home: Home): Promise<void> {
    const root = home.layout.gifts;
    directoryRoot(root);
    for (const name of await readdir(root))
        await rm(join(root, name), { recursive: true, force: true });
}

type Upload = {
    stage?: string;
    to: Address;
    agent: AgentRef;
    id: string;
    upload: string;
    owner: AbortSignal;
    files: Map<string, { size: number; offset: number }>;
    bytes: number;
    timer?: NodeJS.Timeout;
    abort: () => void;
    phase: "receiving" | "delivering" | "aborted";
    busy: Promise<unknown>;
    cleaned?: Promise<void>;
};

/** Connection-owned file gifts staged outside every house until all bytes arrive (H25). */
export function uploads(home: Home, context: Context) {
    const pending = new Map<string, Upload>();
    const active = new Set<Upload>();
    let stopped = false;
    const detach = (upload: Upload) => {
        pending.delete(upload.upload);
        clearTimeout(upload.timer);
        upload.owner.removeEventListener("abort", upload.abort);
    };
    const clean = (upload: Upload) => {
        upload.cleaned ??= (async () => {
            try {
                if (upload.stage !== undefined)
                    await rm(upload.stage, { recursive: true, force: true });
            } catch (error) {
                throw giftError(error);
            }
        })();
        return upload.cleaned;
    };
    const drop = async (upload: Upload) => {
        if (upload.phase !== "receiving") return;
        detach(upload);
        upload.phase = "aborted";
        await upload.busy.catch(() => {});
        await clean(upload);
        active.delete(upload);
    };
    const discard = (upload: Upload) => {
        void drop(upload).catch(() =>
            process.stderr.write("hamlet: could not discard a gift.\n"),
        );
    };
    const receiving = (upload: Upload) => {
        if (upload.phase !== "receiving" || upload.owner.aborted)
            throw new Error("The gift's connection closed.");
    };
    const perform = <T>(upload: Upload, step: () => Promise<T>): Promise<T> => {
        const work = upload.busy
            .catch(() => {})
            .then(async () => {
                try {
                    receiving(upload);
                    return await step();
                } catch (error) {
                    detach(upload);
                    upload.phase = "aborted";
                    throw giftError(error);
                } finally {
                    if (upload.phase !== "receiving") {
                        try {
                            await clean(upload);
                        } finally {
                            active.delete(upload);
                        }
                    }
                }
            });
        upload.busy = work;
        return work;
    };
    const touch = (upload: Upload) => {
        clearTimeout(upload.timer);
        upload.timer = setTimeout(() => discard(upload), IDLE_MS);
        upload.timer.unref();
    };
    const finish = async (upload: Upload): Promise<ChiefResult<string>> => {
        receiving(upload);
        upload.phase = "delivering";
        detach(upload);
        const target = await resolved(home, upload.to, context);
        if (!target.ok) return target;
        const agent = target.value.ref;
        if (agent.number !== upload.agent.number)
            throw new Error("The receiving agent is no longer present.");
        const roots = [
            ...new Set(
                [...upload.files.keys()].map(
                    (path) => path.split("/")[0] ?? "",
                ),
            ),
        ];
        const paths = (
            await deliver(
                home,
                houseOf(home.layout, agent),
                "inbox",
                upload.stage ?? "",
                roots,
                context,
            )
        ).map((name) => `inbox/${name}`);
        await clean(upload);
        await submitLine(
            home,
            { agent, chief: home.chief },
            {
                id: upload.id,
                text: safeWords(
                    home,
                    `I handed you ${paths.map((path) => `\`${path}\``).join(", ")} in your workspace. Read it and do what it asks. If it asks for nothing, tell me in a few lines what it is.`,
                ),
            },
            context,
        );
        return {
            ok: true,
            value: `Handed over ${paths.map((path) => `\`${path}\``).join(", ")}.`,
        };
    };
    return {
        async begin(
            message: Extract<Give, { files: unknown }>,
            owner: AbortSignal,
        ): Promise<Reply> {
            const target = await resolved(home, message.to, context);
            if (!target.ok)
                return {
                    type: "reply",
                    to: message.id,
                    kind: "error",
                    text: chiefRefusalText(target),
                };
            const agent = target.value.ref;
            const bytes = manifestSize(message.files);
            await checkShelf(
                houseOf(home.layout, agent),
                "inbox",
                message.files.map((file) => file.path.split("/")[0] ?? ""),
            );
            if (stopped || owner.aborted)
                throw new Error("The gift's connection closed.");
            if (
                [...active].filter((upload) => upload.owner === owner).length >=
                3
            )
                throw new Error(
                    "This connection already has three file gifts waiting. Finish one before starting another.",
                );
            const reserved = [...active]
                .filter(
                    (upload) =>
                        upload.agent.hamlet === agent.hamlet &&
                        upload.agent.number === agent.number,
                )
                .reduce((total, upload) => total + upload.bytes, 0);
            if (reserved + bytes > MAX_GIFT_BYTES)
                throw new Error(
                    "This agent already has 32 MB of file gifts waiting. Finish or discard one before starting another.",
                );
            const upload: Upload = {
                agent,
                to: message.to,
                id: message.id,
                owner,
                upload: randomUUID(),
                bytes,
                files: new Map(
                    message.files.map((file) => [
                        file.path,
                        { size: file.size, offset: 0 },
                    ]),
                ),
                abort: () => discard(upload),
                phase: "receiving",
                busy: Promise.resolve(),
            };
            pending.set(upload.upload, upload);
            active.add(upload);
            owner.addEventListener("abort", upload.abort, { once: true });
            return perform(upload, async () => {
                upload.stage = await staged(home);
                for (const file of message.files) {
                    receiving(upload);
                    const path = join(upload.stage, file.path);
                    await mkdir(dirname(path), { recursive: true });
                    await (await open(path, "wx")).close();
                }
                receiving(upload);
                if (message.files.every((file) => file.size === 0)) {
                    const outcome = await finish(upload);
                    return {
                        type: "reply",
                        to: message.id,
                        kind: outcome.ok ? "notice" : "error",
                        text: outcome.ok
                            ? outcome.value
                            : chiefRefusalText(outcome),
                    };
                }
                touch(upload);
                return {
                    type: "reply",
                    to: message.id,
                    kind: "notice",
                    text: "Ready for the gift's file parts.",
                    upload: upload.upload,
                };
            });
        },
        async part(
            message: GivePart,
            owner: AbortSignal,
        ): Promise<ChiefResult<string>> {
            const upload = pending.get(message.upload);
            if (upload === undefined || upload.owner !== owner)
                throw new Error(
                    "There is no file gift here with that upload id. It may have expired.",
                );
            return perform(upload, async () => {
                clearTimeout(upload.timer);
                const file = upload.files.get(message.path);
                const bytes = Buffer.from(message.data, "base64");
                if (
                    file === undefined ||
                    message.offset !== file.offset ||
                    bytes.length === 0 ||
                    bytes.toString("base64") !== message.data ||
                    file.offset + bytes.length > file.size
                )
                    throw new Error(
                        "A gift part must match its path, next offset, size and base64 bytes.",
                    );
                const fd = await open(
                    join(upload.stage ?? "", message.path),
                    "a",
                );
                try {
                    await fd.writeFile(bytes);
                } finally {
                    await fd.close();
                }
                file.offset += bytes.length;
                receiving(upload);
                if (
                    [...upload.files.values()].every(
                        (file) => file.offset === file.size,
                    )
                )
                    return finish(upload);
                touch(upload);
                return { ok: true, value: "Received the gift part." };
            });
        },
        async stop() {
            stopped = true;
            await Promise.all(
                [...active].map(async (upload) => {
                    await drop(upload);
                    await upload.busy.catch(() => {});
                }),
            );
        },
    };
}

function giftError(error: unknown): Error {
    return error instanceof Error && !("code" in error)
        ? error
        : new Error(STAGE_REFUSED);
}

/** A chief's skill gift delivered through the same deed as typed /give (H25). */
export async function give(
    home: Home,
    message: Extract<Give, { skill: unknown }>,
    context: Context,
): Promise<ChiefResult<string>> {
    const skill = message.skill;
    return "starter" in skill
        ? giftSkill(home, message.to, skill.starter, undefined, context)
        : giftSkill(home, message.to, skill.name, skill.from, context);
}

/** A skill from the shipped starters or a resolved giver's house (H25). */
export async function giftSkill(
    home: Home,
    to: Address,
    skill: string,
    from: Address | undefined,
    context: Context,
): Promise<ChiefResult<string>> {
    if (!/^[a-zA-Z0-9_-]+$/.test(skill))
        throw new Error("a skill is named in letters, digits and hyphens");
    const target = await resolved(home, to, context);
    if (!target.ok) return target;
    const agent = target.value.ref;
    const house = houseOf(home.layout, agent);
    if (await lstat(join(house, "skills", skill)).catch(() => undefined))
        throw new Error(
            `\`${target.value.name}\` already has a skill named \`${skill}\``,
        );
    let source = STARTERS;
    let shelf = "";
    let giver: string | null = null;
    if (from !== undefined) {
        const given = await resolved(home, from, context);
        if (!given.ok) return given;
        source = houseOf(home.layout, given.value.ref);
        giver = `${given.value.name}${from.hamlet === to.hamlet ? "" : `@${from.hamlet}`}`;
        shelf = "skills";
    }
    if (
        !(
            await lstat(join(source, shelf, skill, "SKILL.md")).catch(
                () => undefined,
            )
        )?.isFile()
    )
        throw new Error(
            from === undefined
                ? `no starter skill named \`${skill}\`. Say whose it is: /give ${target.value.name} ${skill} from <agent>`
                : `\`${from.agent}\` has no skill named \`${skill}\``,
        );
    let stage: string | undefined;
    try {
        if (shelf && !(await lstat(join(source, shelf))).isDirectory())
            throw new Error(
                "A skill gift cannot include links or special files.",
            );
        if (from !== undefined) {
            stage = await staged(home);
            await stageSkill(home, source, skill, stage, context);
        } else await checkSkill(join(source, skill));
        await deliver(home, house, "skills", stage ?? source, [skill], context);
    } catch (error) {
        throw giftError(error);
    } finally {
        if (stage !== undefined)
            await rm(stage, { recursive: true, force: true }).catch((error) => {
                throw giftError(error);
            });
    }
    refreshSkills(house);
    await home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, to.hamlet, to.hamlet)).decisions.push({
            kind: "skill_given",
            at: home.now(),
            by: home.chief,
            to: agent.number,
            skill,
            from: giver,
        });
    }, context);
    return { ok: true, value: `Gave \`${to.agent}\` the skill \`${skill}\`.` };
}
