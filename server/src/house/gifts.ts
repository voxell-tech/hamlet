import { randomUUID } from "node:crypto";
import { lstat, mkdir, mkdtemp, open, readdir, rm } from "node:fs/promises";
import { dirname, join, posix } from "node:path";
import type { Context } from "@earendil-works/chord";
import type { FileError, FileInfo } from "@earendil-works/pi-durable/env";
import { submitLine } from "../chats/conversations.ts";
import { houseOf } from "../home/layout.ts";
import type { Home } from "../home.ts";
import { STARTERS } from "../library/shelf.ts";
import {
    type Address,
    type Give,
    type GivePart,
    MAX_GIFT_BYTES,
} from "../protocol.ts";
import { type AddressFailure, livingAgent } from "../realm/roads.ts";
import { type AgentRef, nameOf, RecordDoc } from "../record.ts";
import { err, ok, type Result } from "../result.ts";
import { directoryRoot } from "../sandbox/policy.ts";
import type { Receipt } from "../server.ts";
import { checkShelf, type GiftRefusal, manifestSize } from "./gift-rules.ts";
import { type RemoteHouse, remoteHouse } from "./remote.ts";
import { deliver, GIFT_CAPS, stage, type TreeFailure } from "./trees.ts";

/** A raw tree failure carries remote facts the gift wording cannot state, such as a partial delivery. */
type GiftResult<T> = Result<T, GiftRefusal | AddressFailure | TreeFailure>;
const IDLE_MS = 60_000;
function refuse(
    kind:
        | "stage-refused"
        | "connection-closed"
        | "uploads-full"
        | "bytes-full"
        | "part-mismatch"
        | "upload-unknown"
        | "recipient-changed",
): Result<never, GiftRefusal> {
    return err({ kind, operation: "give", retrySafe: false });
}
async function staged(home: Home): Promise<Result<string, GiftRefusal>> {
    try {
        const root = home.layout.gifts;
        directoryRoot(root);
        await mkdir(root, { recursive: true, mode: 0o700 });
        directoryRoot(root);
        return ok(await mkdtemp(join(root, "gift-")));
    } catch {
        return refuse("stage-refused");
    }
}
async function resolved(
    home: Home,
    to: Address,
    context: Context,
): Promise<GiftResult<{ ref: AgentRef; name: string }>> {
    const target = await livingAgent(home, to, "give", context);
    if (!target.ok) return target;
    const record = await home.harness.snapshot(RecordDoc, to.hamlet, context);
    if (!record) throw new Error("A resolved gift recipient has no record.");
    return ok({
        ref: target.value.ref,
        name: nameOf(record, target.value.ref.number),
    });
}

/** Remote facts pass through as tree failures; the rest keep the gift's own wording. */
function giftTree(
    kind: "skill-tree" | "gift-tree",
    failure: TreeFailure,
): Result<never, GiftRefusal | TreeFailure> {
    if (
        (failure.kind === "refused" && failure.remote !== undefined) ||
        (failure.kind === "bytes-cap" && failure.scope === "file")
    )
        return err(failure);
    return err({ kind, failure, operation: "give", retrySafe: false });
}
function unreadable(): Result<never, GiftRefusal> {
    return err({
        kind: "gift-tree",
        failure: {
            kind: "refused",
            operation: "deliver-tree",
            retrySafe: false,
        },
        operation: "give",
        retrySafe: false,
    });
}
function unmappable(): Result<never, TreeFailure> {
    return err({
        kind: "refused",
        remote: "unmappable",
        operation: "deliver-tree",
        retrySafe: false,
    });
}

/** Inspects one path of a remote house without following a final link; `null` when it is missing. */
async function remoteInfo(
    house: RemoteHouse,
    rel: string,
    context: Context,
): Promise<Result<FileInfo | null, FileError>> {
    const env = house.open();
    try {
        const info = await env.fileInfo(posix.join(house.path, rel), context);
        if (info.ok) return info;
        return info.error.code === "not_found" ? ok(null) : info;
    } finally {
        await env.cleanup(context);
    }
}

/** The receiving shelf's admission check, in the house's workspace when it is remote. */
async function shelfReady(
    home: Home,
    agent: AgentRef,
    shelf: string,
    roots: string[],
    context: Context,
): Promise<GiftResult<void>> {
    const house = houseOf(home.layout, agent);
    const where = remoteHouse(home, house);
    if (where.kind === "host") return checkShelf(house, shelf, roots);
    if (where.kind === "unmappable") return unmappable();
    const linked = (): GiftResult<never> =>
        err({
            kind: "gift-tree",
            failure: {
                kind: "shelf-linked",
                operation: "deliver-tree",
                retrySafe: false,
            },
            operation: "give",
            retrySafe: false,
        });
    const dir = await remoteInfo(where.house, shelf, context);
    if (!dir.ok) return unreadable();
    if (dir.value !== null && dir.value.kind !== "directory") return linked();
    if (dir.value === null) return ok(undefined);
    for (const root of new Set(roots)) {
        const entry = await remoteInfo(
            where.house,
            posix.join(shelf, root),
            context,
        );
        if (!entry.ok) return unreadable();
        if (entry.value?.kind === "symlink") return linked();
    }
    return ok(undefined);
}

/** Abandoned gifts are removed before the server accepts uploads. */
export async function clearStagedGifts(
    home: Home,
): Promise<Result<void, GiftRefusal>> {
    try {
        const root = home.layout.gifts;
        directoryRoot(root);
        for (const name of await readdir(root))
            await rm(join(root, name), { recursive: true, force: true });
        return ok(undefined);
    } catch {
        return refuse("stage-refused");
    }
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
    cleaned?: Promise<Result<void, GiftRefusal>>;
};

/** Connection-owned file gifts remain outside every house until all bytes arrive. */
export function uploads(home: Home, context: Context) {
    const pending = new Map<string, Upload>();
    const active = new Set<Upload>();
    let stopped = false;
    const detach = (upload: Upload) => {
        pending.delete(upload.upload);
        clearTimeout(upload.timer);
        upload.owner.removeEventListener("abort", upload.abort);
    };
    const clean = (upload: Upload): Promise<Result<void, GiftRefusal>> => {
        upload.cleaned ??= (async () => {
            try {
                if (upload.stage !== undefined)
                    await rm(upload.stage, { recursive: true, force: true });
                return ok(undefined);
            } catch {
                return refuse("stage-refused");
            }
        })();
        return upload.cleaned;
    };
    const drop = async (upload: Upload): Promise<Result<void, GiftRefusal>> => {
        if (upload.phase !== "receiving") return ok(undefined);
        detach(upload);
        upload.phase = "aborted";
        await upload.busy.catch(() => {});
        const result = await clean(upload);
        active.delete(upload);
        return result;
    };
    const discard = (upload: Upload) => {
        void drop(upload).then((outcome) => {
            if (!outcome.ok)
                process.stderr.write("hamlet: could not discard a gift.\n");
        });
    };
    const receiving = (upload: Upload): Result<void, GiftRefusal> =>
        upload.phase !== "receiving" || upload.owner.aborted
            ? refuse("connection-closed")
            : ok(undefined);
    const perform = <T>(
        upload: Upload,
        step: () => Promise<GiftResult<T>>,
    ): Promise<GiftResult<T>> => {
        const work = upload.busy
            .catch(() => {})
            .then(async (): Promise<GiftResult<T>> => {
                let outcome: GiftResult<T>;
                try {
                    const ready = receiving(upload);
                    outcome = ready.ok ? await step() : ready;
                } catch (cause) {
                    detach(upload);
                    upload.phase = "aborted";
                    await clean(upload);
                    active.delete(upload);
                    throw cause;
                }
                if (!outcome.ok) {
                    detach(upload);
                    upload.phase = "aborted";
                }
                if (upload.phase !== "receiving") {
                    const cleaned = await clean(upload);
                    active.delete(upload);
                    if (!cleaned.ok) return cleaned;
                }
                return outcome;
            });
        upload.busy = work;
        return work;
    };
    const touch = (upload: Upload) => {
        clearTimeout(upload.timer);
        upload.timer = setTimeout(() => discard(upload), IDLE_MS);
        upload.timer.unref();
    };
    const finish = async (upload: Upload): Promise<GiftResult<string>> => {
        const ready = receiving(upload);
        if (!ready.ok) return ready;
        upload.phase = "delivering";
        detach(upload);
        const target = await resolved(home, upload.to, context);
        if (!target.ok) return target;
        const agent = target.value.ref;
        if (agent.number !== upload.agent.number)
            return refuse("recipient-changed");
        const roots = [
            ...new Set(
                [...upload.files.keys()].map(
                    (path) => path.split("/")[0] ?? "",
                ),
            ),
        ];
        const result = await deliver(
            home,
            {
                root: houseOf(home.layout, agent),
                into: "inbox",
                collision: "number",
            },
            upload.stage ?? "",
            roots,
            context,
        );
        if (!result.ok) return giftTree("gift-tree", result.error);
        const paths = result.value.map((name) => `inbox/${name}`);
        const cleaned = await clean(upload);
        if (!cleaned.ok) return cleaned;
        await submitLine(
            home,
            { agent, chief: home.chief },
            {
                id: upload.id,
                text: home.secrets.words(
                    `I handed you ${paths.map((path) => `\`${path}\``).join(", ")} in your workspace. Read it and do what it asks. If it asks for nothing, tell me in a few lines what it is.`,
                ),
            },
            context,
        );
        return ok(
            `Handed over ${paths.map((path) => `\`${path}\``).join(", ")}.`,
        );
    };
    return {
        async begin(
            message: Extract<Give, { files: unknown }>,
            owner: AbortSignal,
        ): Promise<GiftResult<Receipt>> {
            const target = await resolved(home, message.to, context);
            if (!target.ok) return target;
            const agent = target.value.ref;
            const sized = manifestSize(message.files);
            if (!sized.ok) return sized;
            const checked = await shelfReady(
                home,
                agent,
                "inbox",
                message.files.map((file) => file.path.split("/")[0] ?? ""),
                context,
            );
            if (!checked.ok) return checked;
            if (stopped || owner.aborted) return refuse("connection-closed");
            if (
                [...active].filter((upload) => upload.owner === owner).length >=
                3
            )
                return refuse("uploads-full");
            const reserved = [...active]
                .filter(
                    (upload) =>
                        upload.agent.hamlet === agent.hamlet &&
                        upload.agent.number === agent.number,
                )
                .reduce((total, upload) => total + upload.bytes, 0);
            if (reserved + sized.value > MAX_GIFT_BYTES)
                return refuse("bytes-full");
            const upload: Upload = {
                agent,
                to: message.to,
                id: message.id,
                owner,
                upload: randomUUID(),
                bytes: sized.value,
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
                const stage = await staged(home);
                if (!stage.ok) return stage;
                upload.stage = stage.value;
                for (const file of message.files) {
                    const ready = receiving(upload);
                    if (!ready.ok) return ready;
                    const path = join(stage.value, file.path);
                    try {
                        await mkdir(dirname(path), { recursive: true });
                        await (await open(path, "wx")).close();
                    } catch {
                        return refuse("stage-refused");
                    }
                }
                const ready = receiving(upload);
                if (!ready.ok) return ready;
                if (message.files.every((file) => file.size === 0)) {
                    const outcome = await finish(upload);
                    return outcome.ok ? ok({ text: outcome.value }) : outcome;
                }
                touch(upload);
                return ok({
                    text: "Ready for the gift's file parts.",
                    upload: upload.upload,
                });
            });
        },
        async part(
            message: GivePart,
            owner: AbortSignal,
        ): Promise<GiftResult<string>> {
            const upload = pending.get(message.upload);
            if (upload === undefined || upload.owner !== owner)
                return refuse("upload-unknown");
            return perform(upload, async () => {
                clearTimeout(upload.timer);
                const file = upload.files.get(message.path),
                    bytes = Buffer.from(message.data, "base64");
                if (
                    file === undefined ||
                    message.offset !== file.offset ||
                    !bytes.length ||
                    bytes.toString("base64") !== message.data ||
                    file.offset + bytes.length > file.size
                )
                    return refuse("part-mismatch");
                try {
                    const fd = await open(
                        join(upload.stage ?? "", message.path),
                        "a",
                    );
                    try {
                        await fd.writeFile(bytes);
                    } finally {
                        await fd.close();
                    }
                } catch {
                    return refuse("stage-refused");
                }
                file.offset += bytes.length;
                const ready = receiving(upload);
                if (!ready.ok) return ready;
                if (
                    [...upload.files.values()].every(
                        (file) => file.offset === file.size,
                    )
                )
                    return finish(upload);
                touch(upload);
                return ok("Received the gift part.");
            });
        },
        async stop(): Promise<Result<void, GiftRefusal>> {
            stopped = true;
            const results = await Promise.all(
                [...active].map(async (upload) => {
                    const result = await drop(upload);
                    await upload.busy.catch(() => {});
                    return result;
                }),
            );
            for (const result of results) if (!result.ok) return result;
            return ok(undefined);
        },
    };
}

/** A chief's skill gift uses the same admission as typed /give. */
export async function give(
    home: Home,
    message: Extract<Give, { skill: unknown }>,
    context: Context,
): Promise<GiftResult<string>> {
    return "starter" in message.skill
        ? giftSkill(home, message.to, message.skill.starter, undefined, context)
        : giftSkill(
              home,
              message.to,
              message.skill.name,
              message.skill.from,
              context,
          );
}

/** A starter or another agent's skill, copied through a no-follow private stage. */
export async function giftSkill(
    home: Home,
    to: Address,
    skill: string,
    from: Address | undefined,
    context: Context,
): Promise<GiftResult<string>> {
    if (!/^[a-zA-Z0-9_-]+$/.test(skill))
        return err({ kind: "skill-name", operation: "give", retrySafe: false });
    const target = await resolved(home, to, context);
    if (!target.ok) return target;
    const agent = target.value.ref,
        house = houseOf(home.layout, agent);
    const receiving = remoteHouse(home, house);
    if (receiving.kind === "unmappable") return unmappable();
    let present: boolean;
    if (receiving.kind === "remote") {
        const found = await remoteInfo(
            receiving.house,
            `skills/${skill}`,
            context,
        );
        if (!found.ok) return unreadable();
        present = found.value !== null;
    } else
        present =
            (await lstat(join(house, "skills", skill)).catch(
                () => undefined,
            )) !== undefined;
    if (present)
        return err({
            kind: "skill-exists",
            agent: target.value.name,
            skill,
            operation: "give",
            retrySafe: false,
        });
    let source = STARTERS,
        shelf = "",
        giver: string | null = null;
    if (from !== undefined) {
        const given = await resolved(home, from, context);
        if (!given.ok) return given;
        source = houseOf(home.layout, given.value.ref);
        giver = `${given.value.name}${from.hamlet === to.hamlet ? "" : `@${from.hamlet}`}`;
        shelf = "skills";
    }
    const giving = remoteHouse(home, source);
    if (giving.kind === "unmappable") return unmappable();
    let paper: boolean;
    if (giving.kind === "remote") {
        const found = await remoteInfo(
            giving.house,
            `${shelf}/${skill}/SKILL.md`,
            context,
        );
        if (!found.ok) return unreadable();
        paper = found.value?.kind === "file";
    } else
        paper =
            (
                await lstat(join(source, shelf, skill, "SKILL.md")).catch(
                    () => undefined,
                )
            )?.isFile() === true;
    if (!paper)
        return err({
            kind: "skill-missing",
            agent: from?.agent ?? "",
            skill,
            starter: from === undefined,
            target: target.value.name,
            operation: "give",
            retrySafe: false,
        });
    const temporary = await staged(home);
    if (!temporary.ok) return temporary;
    let outcome: GiftResult<string>;
    let cleaned: Result<void, GiftRefusal> = ok(undefined);
    try {
        const copied = await stage(
            home,
            { root: source, rel: shelf ? `${shelf}/${skill}` : skill },
            temporary.value,
            GIFT_CAPS,
            context,
        );
        if (!copied.ok) outcome = giftTree("skill-tree", copied.error);
        else {
            const result = await deliver(
                home,
                { root: house, into: "skills" },
                temporary.value,
                [skill],
                context,
            );
            if (!result.ok) outcome = giftTree("gift-tree", result.error);
            else {
                if (receiving.kind === "remote")
                    home.houses.of(agent).invalidate();
                else home.houses.invalidate(house);
                await home.harness.commit(async (tx) => {
                    (
                        await tx.doc(RecordDoc, to.hamlet, to.hamlet)
                    ).decisions.push({
                        kind: "skill_given",
                        at: home.now(),
                        by: home.chief,
                        to: agent.number,
                        skill,
                        from: giver,
                    });
                }, context);
                outcome = ok(`Gave \`${to.agent}\` the skill \`${skill}\`.`);
            }
        }
    } finally {
        try {
            await rm(temporary.value, { recursive: true, force: true });
        } catch {
            cleaned = refuse("stage-refused");
        }
    }
    return cleaned.ok ? outcome : cleaned;
}
