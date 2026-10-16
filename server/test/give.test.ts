import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { constants } from "node:fs";
import {
    chmod,
    lstat,
    mkdir,
    open,
    readdir,
    readFile,
    rename,
    rm,
    symlink,
    writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import { ExecutionError, err } from "@earendil-works/pi-durable/env";
import { expect, it, vi } from "vitest";
import { type Connection, connect, serve } from "../src/connection.ts";
import { buildHouse, houseOf } from "../src/home.ts";
import { scriptedModels } from "../src/models.ts";
import {
    type ClientMessage,
    parseClient,
    parseServer,
    type ServerMessage,
} from "../src/protocol.ts";
import { RecordDoc } from "../src/record.ts";
import { SandboxEnv } from "../src/sandbox.ts";
import { safeValue } from "../src/scrub.ts";
import {
    agents,
    chat,
    joined,
    mayor,
    request as sendRequest,
    served,
} from "./connection-support.ts";
import { swappingDirectory } from "./race-support.ts";
import {
    addHamlet,
    closeAfter,
    context,
    fauxModels,
    MAYOR,
    mayorChat,
    scratch,
    transcript,
} from "./support.ts";

async function request(
    connection: Connection,
    messages: ServerMessage[],
    message:
        | (ClientMessage & { id: string })
        | {
              type: "give";
              id: string;
              to: typeof mayor;
              files: { path: string; data: string }[];
          },
) {
    if (
        message.type !== "give" ||
        !("files" in message) ||
        message.files.some((file) => !("data" in file))
    )
        return sendRequest(
            connection,
            messages,
            message as ClientMessage & { id: string },
        );
    const files = message.files as { path: string; data: string }[];
    let reply = await sendRequest(connection, messages, {
        ...message,
        files: files.map((file) => ({
            path: file.path,
            size: Buffer.from(file.data, "base64").length,
        })),
    });
    if (reply.type !== "reply" || reply.upload === undefined) return reply;
    const upload = reply.upload;
    for (const file of files) {
        const bytes = Buffer.from(file.data, "base64");
        for (let offset = 0; offset < bytes.length; offset += 512 * 1024) {
            reply = await sendRequest(connection, messages, {
                type: "give-part",
                id: randomUUID(),
                upload,
                path: file.path,
                offset,
                data:
                    bytes.toString("base64") === file.data
                        ? bytes
                              .subarray(offset, offset + 512 * 1024)
                              .toString("base64")
                        : file.data,
            });
            if (reply.type === "reply" && reply.kind === "error") return reply;
        }
    }
    return reply;
}

it("hands the mayor a file over the connection with its bytes and a named receipt", async () => {
    const { models } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    const bytes = Buffer.from([0, 255, 1, 128, 10]);
    const id = randomUUID();
    const reply = await request(run.connection, run.messages, {
        type: "give",
        id,
        to: mayor,
        files: [{ path: "list.bin", data: bytes.toString("base64") }],
    });
    expect(reply).toMatchObject({
        type: "reply",
        to: expect.any(String),
        kind: "notice",
        text: expect.stringContaining("inbox/list.bin"),
    });
    expect(
        await readFile(join(houseOf(run.home.layout, MAYOR), "inbox/list.bin")),
    ).toEqual(bytes);
});

it("keeps a folder's structure and earlier gifts, and tells the agent where it landed", async () => {
    const { models } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    const give = () =>
        request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [
                {
                    path: "folder/deep/a.md",
                    data: Buffer.from("A").toString("base64"),
                },
                {
                    path: "folder/b.md",
                    data: Buffer.from("B").toString("base64"),
                },
            ],
        });
    expect(await give()).toMatchObject({
        type: "reply",
        kind: "notice",
        text: expect.stringContaining("inbox/folder"),
    });
    expect(await give()).toMatchObject({
        type: "reply",
        kind: "notice",
        text: expect.stringContaining("inbox/folder-2"),
    });
    const inbox = join(houseOf(run.home.layout, MAYOR), "inbox");
    for (const folder of ["folder", "folder-2"]) {
        expect(await readFile(join(inbox, folder, "deep/a.md"), "utf8")).toBe(
            "A",
        );
        expect(await readFile(join(inbox, folder, "b.md"), "utf8")).toBe("B");
    }
    const transcript = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat,
    });
    expect(transcript).toMatchObject({
        type: "transcript",
        items: expect.arrayContaining([
            expect.objectContaining({
                kind: "user",
                text: "I handed you `inbox/folder-2` in your workspace. Read it and do what it asks. If it asks for nothing, tell me in a few lines what it is.",
            }),
        ]),
    });
});

it.each([
    "/absolute.txt",
    "../outside.txt",
    "folder/../outside.txt",
    "C:\\outside.txt",
    "folder\\..\\outside.txt",
    "",
    ".",
    "folder//a",
    "nul\0.txt",
])("refuses the path %j before writing any part of a gift", async (path) => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    const reply = await request(run.connection, run.messages, {
        type: "give",
        id: randomUUID(),
        to: mayor,
        files: [
            { path: "innocent.txt", data: "QQ==" },
            { path, data: "Qg==" },
        ],
    });
    expect(reply).toMatchObject({
        type: "reply",
        kind: "error",
        text: expect.stringMatching(/path|inbox/),
    });
    expect(
        await readdir(join(houseOf(run.home.layout, MAYOR), "inbox")),
    ).toEqual([]);
    expect(faux.state.callCount).toBe(0);
});

it.each([false, true])(
    "refuses a path through a link leaving the inbox (root link %j)",
    async (root) => {
        const { models } = fauxModels();
        const run = await served({ models });
        const inbox = join(houseOf(run.home.layout, MAYOR), "inbox");
        const outside = scratch();
        if (root) await rename(inbox, `${inbox}-kept`);
        await symlink(outside, root ? inbox : join(inbox, "escape"));
        const reply = await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [
                { path: "innocent.txt", data: "QQ==" },
                { path: "escape/out.txt", data: "Qg==" },
            ],
        });
        expect(reply).toMatchObject({
            type: "reply",
            kind: "error",
            text: expect.stringContaining("inbox"),
        });
        expect(await readdir(outside)).toEqual([]);
        if (!root) expect(await readdir(inbox)).toEqual(["escape"]);
    },
);

it.each([
    { files: [] },
    { files: [{ path: "a", data: "not base64!" }] },
    {
        files: [
            { path: "a", data: "QQ==" },
            { path: "a", data: "Qg==" },
        ],
    },
    {
        files: [
            { path: "a/b", data: "QQ==" },
            { path: "a", data: "Qg==" },
        ],
    },
])(
    "refuses an empty, malformed or conflicting gift without writing",
    async ({ files }) => {
        const { models } = fauxModels();
        const run = await served({ models });
        expect(
            await request(run.connection, run.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                files,
            }),
        ).toMatchObject({
            type: "reply",
            kind: "error",
            text: expect.any(String),
        });
        expect(
            await readdir(join(houseOf(run.home.layout, MAYOR), "inbox")),
        ).toEqual([]);
    },
);

it("lists starters and house skills, installs a starter and sends the changed skills to every window", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    const other = await joined(run.server.url, run.home.credentials.token);
    expect(run.state.starters).toContainEqual({
        name: "web-search",
        description: expect.stringMatching(/\S/),
    });
    expect(run.state.agents[0]?.skills).toEqual([]);
    const reply = await request(run.connection, run.messages, {
        type: "give",
        id: randomUUID(),
        to: mayor,
        skill: { starter: "web-search" },
    });
    expect(reply).toMatchObject({
        type: "reply",
        kind: "notice",
        text: "Gave `mayor` the skill `web-search`.",
    });
    const skill = join(houseOf(run.home.layout, MAYOR), "skills/web-search");
    for (const file of ["SKILL.md", "scripts/search.py"]) {
        expect(await readFile(join(skill, file))).toEqual(
            await readFile(
                new URL(
                    `../assets/starters/web-search/${file}`,
                    import.meta.url,
                ),
            ),
        );
    }
    expect(agents(run.messages).at(-1)?.skills).toEqual(["web-search"]);
    await expect
        .poll(() => agents(other.messages).at(-1)?.skills)
        .toEqual(["web-search"]);
    const next = await joined(run.server.url, run.home.credentials.token);
    expect(next.state.agents[0]?.skills).toEqual(["web-search"]);
    expect(
        (await run.home.harness.snapshot(RecordDoc, "home", context))
            ?.decisions,
    ).toContainEqual({
        kind: "skill_given",
        at: expect.any(Number),
        by: "chief",
        to: 0,
        skill: "web-search",
        from: null,
    });
    expect(
        await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            skill: { starter: "web-search" },
        }),
    ).toMatchObject({
        type: "reply",
        kind: "error",
        text: "`founder` already has a skill named `web-search`",
    });
    expect(
        await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            skill: { starter: "nothing" },
        }),
    ).toMatchObject({
        type: "reply",
        kind: "error",
        text: "no starter skill named `nothing`. Say whose it is: /give founder nothing from <agent>",
    });
    expect(faux.state.callCount).toBe(0);
});

it("copies another agent's whole skill, keeps the giver's and records its source", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    const source = await buildHouse(run.home.layout, {
        hamlet: "home",
        number: 1,
    });
    await mkdir(join(source, "skills/maps/scripts"), { recursive: true });
    await writeFile(
        join(source, "skills/maps/SKILL.md"),
        "---\nname: maps\ndescription: Read maps.\n---\nA map.",
    );
    await writeFile(join(source, "skills/maps/scripts/run.sh"), "echo maps");
    await chmod(join(source, "skills/maps/scripts/run.sh"), 0o755);
    await run.home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, "home", "home")).agents.push({
            number: 1,
            name: "scout",
            purpose: "Maps",
            model: null,
            since: 1,
        });
    }, context);
    const reply = await request(run.connection, run.messages, {
        type: "give",
        id: randomUUID(),
        to: mayor,
        skill: { from: { hamlet: "home", agent: "scout" }, name: "maps" },
    });
    expect(reply).toMatchObject({
        type: "reply",
        kind: "notice",
        text: "Gave `mayor` the skill `maps`.",
    });
    expect(
        (
            await lstat(
                join(
                    houseOf(run.home.layout, MAYOR),
                    "skills/maps/scripts/run.sh",
                ),
            )
        ).mode & 0o7777,
    ).toBe(0o755);
    for (const file of ["SKILL.md", "scripts/run.sh"]) {
        expect(
            await readFile(
                join(houseOf(run.home.layout, MAYOR), "skills/maps", file),
            ),
        ).toEqual(await readFile(join(source, "skills/maps", file)));
    }
    expect(
        (await run.home.harness.snapshot(RecordDoc, "home", context))
            ?.decisions,
    ).toContainEqual({
        kind: "skill_given",
        at: expect.any(Number),
        by: "chief",
        to: 0,
        skill: "maps",
        from: "scout",
    });
    expect(
        agents(run.messages).findLast((agent) => agent.name === "founder")
            ?.skills,
    ).toEqual(["maps"]);
    expect(
        await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            skill: {
                from: { hamlet: "home", agent: "scout" },
                name: "missing",
            },
        }),
    ).toMatchObject({
        type: "reply",
        kind: "error",
        text: "`scout` has no skill named `missing`",
    });
    expect(faux.state.callCount).toBe(0);
});

it.each(["file", "starter"])(
    "refuses a %s gift to an agent that left even while its house remains",
    async (kind) => {
        const { models, faux } = fauxModels();
        const run = await served({ models });
        await run.home.harness.commit(async (tx) => {
            (await tx.doc(RecordDoc, "home", "home")).agents = [];
        }, context);
        const gift =
            kind === "file"
                ? { files: [{ path: "a.txt", data: "QQ==" }] }
                : { skill: { starter: "web-search" } };
        expect(
            await request(run.connection, run.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                ...gift,
            }),
        ).toMatchObject({
            type: "reply",
            kind: "error",
            text: "no agent named `mayor`",
        });
        expect(
            await readdir(join(houseOf(run.home.layout, MAYOR), "inbox")),
        ).toEqual([]);
        expect(
            await readdir(join(houseOf(run.home.layout, MAYOR), "skills")),
        ).toEqual([]);
        expect(faux.state.callCount).toBe(0);
    },
);

it("keeps simultaneous gifts beside one another without overwriting bytes", async () => {
    const { models } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    const other = await joined(run.server.url, run.home.credentials.token);
    const replies = await Promise.all(
        [run, other].map((client, n) =>
            request(client.connection, client.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                files: [
                    {
                        path: "list.txt",
                        data: Buffer.from(n === 0 ? "Dune" : "Emma").toString(
                            "base64",
                        ),
                    },
                ],
            }),
        ),
    );
    expect(replies).toEqual(
        expect.arrayContaining([
            expect.objectContaining({
                kind: "notice",
                text: "Handed over `inbox/list.txt`.",
            }),
            expect.objectContaining({
                kind: "notice",
                text: "Handed over `inbox/list-2.txt`.",
            }),
        ]),
    );
    const inbox = join(houseOf(run.home.layout, MAYOR), "inbox");
    const bytes = await Promise.all(
        ["list.txt", "list-2.txt"].map((file) =>
            readFile(join(inbox, file), "utf8"),
        ),
    );
    expect(bytes.sort()).toEqual(["Dune", "Emma"]);
});

it.each(["skill", "nested"])(
    "refuses a skill with a linked %s instead of copying files beyond the giver's house",
    async (linked) => {
        const { models } = fauxModels();
        const run = await served({ models });
        const source = await buildHouse(run.home.layout, {
            hamlet: "home",
            number: 1,
        });
        const outside = scratch();
        await writeFile(join(outside, "SKILL.md"), "Outside");
        if (linked === "skill")
            await symlink(outside, join(source, "skills/maps"));
        else {
            await mkdir(join(source, "skills/maps"));
            await writeFile(
                join(source, "skills/maps/SKILL.md"),
                "---\nname: maps\n---\n",
            );
            await symlink(outside, join(source, "skills/maps/scripts"));
        }
        await run.home.harness.commit(async (tx) => {
            (await tx.doc(RecordDoc, "home", "home")).agents.push({
                number: 1,
                name: "scout",
                purpose: "Maps",
                model: null,
                since: 1,
            });
        }, context);
        expect(
            await request(run.connection, run.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                skill: {
                    from: { hamlet: "home", agent: "scout" },
                    name: "maps",
                },
            }),
        ).toMatchObject({
            type: "reply",
            kind: "error",
            text: "A skill gift cannot include links or special files.",
        });
        expect(
            await readdir(join(houseOf(run.home.layout, MAYOR), "skills")),
        ).toEqual([]);
        expect(
            await request(run.connection, run.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                skill: { starter: "weather" },
            }),
        ).toMatchObject({
            kind: "notice",
            text: "Gave `mayor` the skill `weather`.",
        });
    },
);

it.each(["skills", "skills/maps"])(
    "refuses a giver's %s swapped for an outside link at copy time",
    async (part) => {
        const { models } = fauxModels();
        const run = await served({ models });
        const source = await scout(run);
        const outside = scratch();
        const folder = part === "skills" ? join(outside, "maps") : outside;
        await mkdir(folder, { recursive: true });
        await writeFile(join(folder, "SKILL.md"), "---\nname: maps\n---\n");
        await writeFile(join(folder, "id_rsa"), "Outside every house");
        const original = SandboxEnv.prototype.exec;
        let swapped = false;
        const copying = vi
            .spyOn(SandboxEnv.prototype, "exec")
            .mockImplementation(async function (
                this: SandboxEnv,
                command,
                options,
                context,
            ) {
                if (
                    Array.isArray(command) &&
                    command.includes(join(source, "skills/maps")) &&
                    !swapped
                ) {
                    swapped = true;
                    const path = join(source, part);
                    await rename(path, `${path}-kept`);
                    await symlink(outside, path);
                }
                return original.call(this, command, options, context);
            });
        try {
            const reply = await request(run.connection, run.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                skill: {
                    from: { hamlet: "home", agent: "scout" },
                    name: "maps",
                },
            });
            expect(swapped).toBe(true);
            expect(reply).toMatchObject({
                kind: "error",
                text: expect.stringMatching(
                    /skill gift|could not be delivered/,
                ),
            });
            expect(JSON.stringify(reply)).not.toContain(outside);
            expect(
                await readdir(join(houseOf(run.home.layout, MAYOR), "skills")),
            ).toEqual([]);
            expect(
                await readdir(join(run.home.layout.dir, ".gifts")).catch(
                    () => [],
                ),
            ).toEqual([]);
            expect(
                (await run.home.harness.snapshot(RecordDoc, "home", context))
                    ?.decisions,
            ).not.toContainEqual(
                expect.objectContaining({ kind: "skill_given" }),
            );
        } finally {
            copying.mockRestore();
        }
    },
);

it.each(
    ["skill", "file", "folder"].flatMap((gift) =>
        ["folder", "file"].map((kind) => ({ gift, kind })),
    ),
)(
    "preserves an agent's $kind created at the $gift destination at copy time",
    async ({ gift, kind }) => {
        const { models } = fauxModels();
        const run = await served({ models });
        if (gift === "skill") await scout(run);
        const target = join(
            houseOf(run.home.layout, MAYOR),
            gift === "skill" ? "skills/maps" : "inbox/parcel",
        );
        const kept = kind === "folder" ? join(target, "mine") : target;
        const original = SandboxEnv.prototype.exec;
        let created = false;
        const copying = vi
            .spyOn(SandboxEnv.prototype, "exec")
            .mockImplementation(async function (
                this: SandboxEnv,
                command,
                options,
                context,
            ) {
                if (
                    Array.isArray(command) &&
                    command.includes(target) &&
                    !created
                ) {
                    created = true;
                    if (kind === "folder") await mkdir(target);
                    await writeFile(kept, "The agent's own work");
                }
                return original.call(this, command, options, context);
            });
        try {
            expect(
                await request(run.connection, run.messages, {
                    type: "give",
                    id: randomUUID(),
                    to: mayor,
                    ...(gift === "skill"
                        ? {
                              skill: {
                                  from: { hamlet: "home", agent: "scout" },
                                  name: "maps",
                              },
                          }
                        : {
                              files: [
                                  {
                                      path:
                                          gift === "file"
                                              ? "parcel"
                                              : "parcel/a",
                                      size: 0,
                                  },
                              ],
                          }),
                }),
            ).toMatchObject({
                kind: "error",
                text: "The gift could not be delivered inside the inbox or skills folder. Nothing was handed over.",
            });
            expect(created).toBe(true);
            expect(await readFile(kept, "utf8")).toBe("The agent's own work");
            if (kind === "folder")
                expect(await readdir(target)).toEqual(["mine"]);
            expect(await readdir(join(run.home.layout.dir, ".gifts"))).toEqual(
                [],
            );
        } finally {
            copying.mockRestore();
        }
    },
);

it("rolls back claimed destinations when the copy's execution reports failure after writing", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    const inbox = join(houseOf(run.home.layout, MAYOR), "inbox");
    const original = SandboxEnv.prototype.exec;
    let failed = false;
    const copying = vi
        .spyOn(SandboxEnv.prototype, "exec")
        .mockImplementation(async function (
            this: SandboxEnv,
            command,
            options,
            context,
        ) {
            const copied = await original.call(this, command, options, context);
            if (Array.isArray(command) && command.includes(inbox) && !failed) {
                failed = true;
                return err(new ExecutionError("timeout", "copy timed out"));
            }
            return copied;
        });
    try {
        expect(
            await request(run.connection, run.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                files: [
                    { path: "first", size: 0 },
                    { path: "folder/last", size: 0 },
                ],
            }),
        ).toMatchObject({
            kind: "error",
            text: "The gift could not be delivered inside the inbox or skills folder. Nothing was handed over.",
        });
        expect(failed).toBe(true);
        expect(await readdir(inbox)).toEqual([]);
        expect(await readdir(join(run.home.layout.dir, ".gifts"))).toEqual([]);
        expect(faux.state.callCount).toBe(0);
    } finally {
        copying.mockRestore();
    }
});

it.each(["link", "pipe", "large", "many"])(
    "refuses a skill that acquires a %s at copy time, before handing its staged copy over",
    async (kind) => {
        const { models } = fauxModels();
        const run = await served({ models });
        const source = await scout(run);
        const folder = join(source, "skills/maps");
        const original = SandboxEnv.prototype.exec;
        let changed = false;
        const copying = vi
            .spyOn(SandboxEnv.prototype, "exec")
            .mockImplementation(async function (
                this: SandboxEnv,
                command,
                options,
                context,
            ) {
                if (
                    Array.isArray(command) &&
                    command.includes(folder) &&
                    !changed
                ) {
                    changed = true;
                    if (kind === "link") {
                        const outside = join(scratch(), "secret");
                        await writeFile(outside, "Outside every house");
                        await symlink(outside, join(folder, "secret"));
                    } else if (kind === "pipe")
                        execFileSync("mkfifo", [join(folder, "pipe")]);
                    else if (kind === "large") {
                        const file = await open(join(folder, "sparse"), "w");
                        await file.truncate(32 * 1024 * 1024 + 1);
                        await file.close();
                    } else
                        for (let n = 0; n < 4095; n++)
                            await mkdir(join(folder, `dir-${n}`));
                }
                return original.call(this, command, options, context);
            });
        try {
            const reply = await request(run.connection, run.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                skill: {
                    from: { hamlet: "home", agent: "scout" },
                    name: "maps",
                },
            });
            expect(changed).toBe(true);
            expect(reply).toMatchObject({
                kind: "error",
                text:
                    kind === "large"
                        ? "The skill gift is larger than 32 MB."
                        : kind === "many"
                          ? "A skill gift takes at most 4096 files and folders at once."
                          : "A skill gift cannot include links or special files.",
            });
            expect(
                await readdir(join(houseOf(run.home.layout, MAYOR), "skills")),
            ).toEqual([]);
            expect(await readdir(join(run.home.layout.dir, ".gifts"))).toEqual(
                [],
            );
            expect(
                (await run.home.harness.snapshot(RecordDoc, "home", context))
                    ?.decisions,
            ).not.toContainEqual(
                expect.objectContaining({ kind: "skill_given" }),
            );
        } finally {
            copying.mockRestore();
        }
    },
    15_000,
);

async function scout(run: Awaited<ReturnType<typeof served>>): Promise<string> {
    const source = await buildHouse(run.home.layout, {
        hamlet: "home",
        number: 1,
    });
    await mkdir(join(source, "skills/maps"));
    await writeFile(
        join(source, "skills/maps/SKILL.md"),
        "---\nname: maps\n---\n",
    );
    await run.home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, "home", "home")).agents.push({
            number: 1,
            name: "scout",
            purpose: "Maps",
            model: null,
            since: 1,
        });
    }, context);
    return source;
}

type Frame =
    | { from: "client"; message: ClientMessage }
    | { from: "server"; message: ServerMessage };

it("keeps a recorded give session in protocol v0 with a gift receipt and skill change", async () => {
    const file = new URL("./frames/give.jsonl", import.meta.url);
    if (process.env.HAMLET_RECORD_FRAMES === "1") {
        const frames = await recordGive();
        await writeFile(
            file,
            `${frames.map((frame) => JSON.stringify(frame)).join("\n")}\n`,
        );
    }
    const frames = (await readFile(file, "utf8"))
        .trimEnd()
        .split("\n")
        .map((line): Frame => JSON.parse(line));
    expect(frames[0]).toEqual({
        from: "client",
        message: { type: "hello", v: 0, token: "[REDACTED]" },
    });
    expect(frames[1]).toMatchObject({
        from: "server",
        message: {
            type: "state",
            starters: expect.arrayContaining([
                expect.objectContaining({ name: "web-search" }),
            ]),
        },
    });
    for (const frame of frames) {
        const parsed = (frame.from === "client" ? parseClient : parseServer)(
            JSON.stringify(frame.message),
        );
        expect(parsed).toHaveProperty("message", frame.message);
    }
    const messages = frames.flatMap((frame) =>
        frame.from === "server" ? [frame.message] : [],
    );
    expect(messages).toContainEqual(
        expect.objectContaining({
            type: "reply",
            kind: "notice",
            text: "Handed over `inbox/folder`.",
        }),
    );
    expect(messages).toContainEqual(
        expect.objectContaining({
            type: "reply",
            kind: "notice",
            text: "Gave `mayor` the skill `web-search`.",
        }),
    );
    expect(agents(messages).at(-1)?.skills).toEqual(["web-search"]);
});

async function recordGive(): Promise<Frame[]> {
    const run = await served({
        models: scriptedModels({ responses: ["A folder received."] }),
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await run.connection.close();
    const wire = await connect(run.server.url);
    closeAfter(() => wire.close());
    const frames: Frame[] = [];
    const connection: Connection = {
        ...wire,
        send(message) {
            frames.push({ from: "client", message: structuredClone(message) });
            wire.send(message);
        },
        async next() {
            const message = await wire.next();
            frames.push({ from: "server", message: structuredClone(message) });
            return message;
        },
    };
    connection.send({ type: "hello", v: 0, token: run.home.credentials.token });
    const messages = [await connection.next()];
    const errors: unknown[] = [];
    const listening = (async () => {
        try {
            for (;;) messages.push(await connection.next());
        } catch (error) {
            errors.push(error);
        }
    })();
    expect(
        await request(connection, messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [
                { path: "folder/deep/a.md", data: "QQ==" },
                { path: "folder/b.md", data: "Qg==" },
            ],
        }),
    ).toMatchObject({ kind: "notice", text: "Handed over `inbox/folder`." });
    expect(
        await readFile(
            join(houseOf(run.home.layout, MAYOR), "inbox/folder/deep/a.md"),
            "utf8",
        ),
    ).toBe("A");
    await expect.poll(() => agents(messages).at(-1)?.busy).toBe(false);
    expect(
        await request(connection, messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            skill: { starter: "web-search" },
        }),
    ).toMatchObject({ kind: "notice" });
    expect(agents(messages).at(-1)?.skills).toEqual(["web-search"]);
    expect(
        await request(connection, messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            skill: { starter: "nothing" },
        }),
    ).toMatchObject({ kind: "error" });
    expect(
        await request(connection, messages, {
            type: "open",
            id: randomUUID(),
            chat,
        }),
    ).toMatchObject({
        type: "transcript",
        items: expect.arrayContaining([
            expect.objectContaining({
                kind: "user",
                text: expect.stringContaining("inbox/folder"),
            }),
            expect.objectContaining({
                kind: "assistant",
                text: "A folder received.",
            }),
        ]),
    });
    await connection.close();
    await listening;
    expect(errors).toHaveLength(1);
    expect(errors[0]).toMatchObject({
        message: "The connection to the server closed (1005).",
    });
    return safeValue(run.home, frames);
}

it("accepts exactly 32 MiB of file bytes through the connection", async () => {
    const { models } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    const bytes = Buffer.alloc(32 * 1024 * 1024, 165);
    expect(
        await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [{ path: "large.bin", data: bytes.toString("base64") }],
        }),
    ).toMatchObject({ kind: "notice", text: "Handed over `inbox/large.bin`." });
    const received = await readFile(
        join(houseOf(run.home.layout, MAYOR), "inbox/large.bin"),
    );
    expect(received.equals(bytes)).toBe(true);
}, 30_000);

it.each([0, 1])(
    "caps every message at 1,048,576 wire bytes (over by %i)",
    async (over) => {
        const { models } = fauxModels();
        const run = await served({ models });
        const server = await serve({
            token: run.home.credentials.token,
            port: 0,
            handlers: {
                hello: () => run.state,
                say: (message) => ({
                    type: "reply",
                    to: message.id,
                    kind: "notice",
                    text: "Heard.",
                }),
            },
        });
        closeAfter(() => server.close());
        const wire = await giftWire({ ...run, server });
        const empty = {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text: "",
        } as const;
        const room =
            1_048_576 + over - Buffer.byteLength(JSON.stringify(empty));
        const message = {
            ...empty,
            text: "\u00e9".repeat(Math.floor(room / 2)) + "x".repeat(room % 2),
        };
        expect(Buffer.byteLength(JSON.stringify(message))).toBe(
            1_048_576 + over,
        );
        wire.send(message);
        if (over === 0)
            expect(await wire.next()).toMatchObject({
                type: "reply",
                to: empty.id,
                kind: "notice",
                text: "Heard.",
            });
        else {
            expect(await wire.next()).toMatchObject({
                type: "fault",
                text: expect.stringMatching(/larger|too large/),
            });
            expect(await wire.closed).toBe(1009);
        }
    },
    30_000,
);

async function giftWire(
    run: Awaited<ReturnType<typeof served>>,
): Promise<Connection> {
    const wire = await connect(run.server.url);
    closeAfter(() => wire.close());
    wire.send({ type: "hello", v: 0, token: run.home.credentials.token });
    expect(await wire.next()).toMatchObject({ type: "state" });
    return wire;
}

it("keeps gift bytes intact but scrubs a credential in its chief notification before keeping it", async () => {
    const { models } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    const secret = run.home.credentials.token;
    const path = `${secret}.txt`;
    expect(
        await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [{ path, data: Buffer.from(secret).toString("base64") }],
        }),
    ).toMatchObject({
        type: "reply",
        kind: "notice",
        text: "Handed over `inbox/[REDACTED].txt`.",
    });
    expect(
        await readFile(
            join(houseOf(run.home.layout, MAYOR), "inbox", path),
            "utf8",
        ),
    ).toBe(secret);
    const kept = await transcript(await mayorChat(run.home));
    expect(JSON.stringify(kept)).not.toContain(secret);
    expect(kept).toContainEqual(
        expect.objectContaining({
            kind: "hamlet.chief",
            data: expect.objectContaining({
                text: expect.stringContaining("inbox/[REDACTED].txt"),
            }),
        }),
    );
});

it("shares a no-follow skill listing, keeping bounded descriptions in papers and prompt but only names in state", async () => {
    const { models } = fauxModels();
    const run = await served({ models });
    const dir = join(houseOf(run.home.layout, MAYOR), "skills");
    for (const name of ["plain", "linked"]) await mkdir(join(dir, name));
    await writeFile(
        join(dir, "plain/SKILL.md"),
        `---\nname: plain\ndescription: A plain skill.\n${"padding\n".repeat(10_000)}usage: echo beyond-the-paper-limit\n---\n`,
    );
    await symlink(join(dir, "plain/SKILL.md"), join(dir, "linked/SKILL.md"));
    const next = await joined(run.server.url, run.home.credentials.token);
    expect(next.state.agents[0]?.skills).toEqual(["plain"]);
    expect(
        await request(run.connection, run.messages, {
            type: "read",
            id: randomUUID(),
            what: "papers",
            agent: mayor,
        }),
    ).toMatchObject({
        papers: { skills: [{ name: "plain", description: "A plain skill." }] },
    });
    const prompt = await request(run.connection, run.messages, {
        type: "read",
        id: randomUUID(),
        what: "prompt",
        agent: mayor,
    });
    expect(prompt).toMatchObject({
        prompt: expect.stringContaining(
            "- plain: A plain skill. (skills/plain/SKILL.md)",
        ),
    });
    expect(JSON.stringify(prompt)).not.toContain("beyond-the-paper-limit");
});

it("refuses a 40 MiB manifest in words addressed to give before receiving bytes", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    const id = randomUUID();
    expect(
        await request(run.connection, run.messages, {
            type: "give",
            id,
            to: mayor,
            files: [{ path: "large.bin", size: 40 * 1024 * 1024 }],
        }),
    ).toMatchObject({
        type: "reply",
        to: id,
        kind: "error",
        text: expect.stringContaining("32 MB"),
    });
    expect(
        await readdir(join(houseOf(run.home.layout, MAYOR), "inbox")),
    ).toEqual([]);
    expect(faux.state.callCount).toBe(0);
});

it.each([
    ["folder/a.txt", "folder/A.txt"],
    ["folder/caf\u00e9.txt", "folder/cafe\u0301.txt"],
    ["folder/stra\u00dfe.txt", "folder/strasse.txt"],
    ["FOLDER", "folder/a.txt"],
])(
    "refuses colliding manifest paths %j and %j without a partial gift",
    async (a, b) => {
        const { models } = fauxModels();
        const run = await served({ models });
        const reply = await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [
                { path: a, size: 1 },
                { path: b, size: 1 },
            ],
        });
        expect(reply).toMatchObject({
            type: "reply",
            kind: "error",
            text: expect.stringMatching(/repeats|both/),
        });
        expect(JSON.stringify(reply)).not.toContain(run.home.layout.dir);
        expect(
            await readdir(join(houseOf(run.home.layout, MAYOR), "inbox")),
        ).toEqual([]);
    },
);

it("gives an agent a skill from the typed giver and records the mayor's resolved name", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    await buildHouse(run.home.layout, { hamlet: "home", number: 1 });
    const source = join(houseOf(run.home.layout, MAYOR), "skills/maps");
    await mkdir(source);
    await writeFile(join(source, "SKILL.md"), "Maps");
    await run.home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, "home", "home")).agents.push({
            number: 1,
            name: "scout",
            purpose: "Maps",
            model: null,
            since: 1,
        });
    }, context);
    expect(
        await request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text: "/give scout maps from mayor",
        }),
    ).toMatchObject({ kind: "notice", text: "Gave `scout` the skill `maps`." });
    expect(
        await readFile(
            join(
                houseOf(run.home.layout, { hamlet: "home", number: 1 }),
                "skills/maps/SKILL.md",
            ),
            "utf8",
        ),
    ).toBe("Maps");
    expect(
        (
            await run.home.harness.snapshot(RecordDoc, "home", context)
        )?.decisions.at(-1),
    ).toMatchObject({ kind: "skill_given", from: "founder" });
    expect(faux.state.callCount).toBe(0);
});

it("keeps a partial upload out of the inbox, drops a bad part and hides server paths", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    const begin = await request(run.connection, run.messages, {
        type: "give",
        id: randomUUID(),
        to: mayor,
        files: [{ path: "folder/a", size: 2 }],
    });
    expect(begin).toMatchObject({ kind: "notice", upload: expect.any(String) });
    if (begin.type !== "reply" || !begin.upload) throw new Error("No upload");
    const part = (offset: number) =>
        request(run.connection, run.messages, {
            type: "give-part",
            id: randomUUID(),
            upload: begin.upload ?? "",
            path: "folder/a",
            offset,
            data: "QQ==",
        });
    expect(await part(0)).toMatchObject({ kind: "notice" });
    const inbox = join(houseOf(run.home.layout, MAYOR), "inbox");
    expect(await readdir(inbox)).toEqual([]);
    const refused = await part(0);
    expect(refused).toMatchObject({
        kind: "error",
        text: expect.stringContaining("offset"),
    });
    expect(JSON.stringify(refused)).not.toContain(run.home.layout.dir);
    expect(await readdir(inbox)).toEqual([]);
    expect(await readdir(join(run.home.layout.dir, ".gifts"))).toEqual([]);
    expect(await part(1)).toMatchObject({
        kind: "error",
        text: expect.stringContaining("upload id"),
    });
    expect(faux.state.callCount).toBe(0);
});

it("hands over zero-byte files only after every file is complete", async () => {
    const { models } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    expect(
        await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [
                { path: "folder/empty", data: "" },
                { path: "folder/a", data: "QQ==" },
            ],
        }),
    ).toMatchObject({ kind: "notice", text: "Handed over `inbox/folder`." });
    expect(
        await readFile(
            join(houseOf(run.home.layout, MAYOR), "inbox/folder/empty"),
        ),
    ).toHaveLength(0);
});

it("drops an idle upload and refuses its late parts", async () => {
    const { models } = fauxModels();
    const run = await served({ models });
    const wire = await giftWire(run);
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    let upload = "";
    try {
        wire.send({
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [{ path: "a", size: 1 }],
        });
        const reply = await wire.next();
        if (reply.type !== "reply" || !reply.upload)
            throw new Error("No upload");
        upload = reply.upload;
        await vi.advanceTimersByTimeAsync(60_001);
    } finally {
        vi.useRealTimers();
    }
    await expect
        .poll(async () => readdir(join(run.home.layout.dir, ".gifts")))
        .toEqual([]);
    expect(
        await request(run.connection, run.messages, {
            type: "give-part",
            id: randomUUID(),
            upload,
            path: "a",
            offset: 0,
            data: "QQ==",
        }),
    ).toMatchObject({
        kind: "error",
        text: expect.stringContaining("expired"),
    });
});

it("drops disconnected uploads and keeps another window from using them", async () => {
    const { models } = fauxModels();
    const run = await served({ models });
    const other = await joined(run.server.url, run.home.credentials.token);
    const reply = await request(other.connection, other.messages, {
        type: "give",
        id: randomUUID(),
        to: mayor,
        files: [{ path: "a", size: 1 }],
    });
    if (reply.type !== "reply" || !reply.upload) throw new Error("No upload");
    expect(
        await request(run.connection, run.messages, {
            type: "give-part",
            id: randomUUID(),
            upload: reply.upload,
            path: "a",
            offset: 0,
            data: "QQ==",
        }),
    ).toMatchObject({ kind: "error" });
    await other.connection.close();
    await expect
        .poll(async () => readdir(join(run.home.layout.dir, ".gifts")))
        .toEqual([]);
    expect(
        await readdir(join(houseOf(run.home.layout, MAYOR), "inbox")),
    ).toEqual([]);
});

it("keeps hello, papers, prompt and gifts responsive beside a FIFO skill paper", async () => {
    const { models } = fauxModels();
    const run = await served({ models });
    const folder = join(houseOf(run.home.layout, MAYOR), "skills/blocked");
    await mkdir(folder);
    const fifo = join(folder, "SKILL.md");
    execFileSync("mkfifo", [fifo]);
    let deadline: NodeJS.Timeout | undefined;
    try {
        await Promise.race([
            (async () => {
                const next = await joined(
                    run.server.url,
                    run.home.credentials.token,
                );
                expect(next.state.agents[0]?.skills).toEqual([]);
                for (const what of ["papers", "prompt"] as const)
                    expect(
                        await request(run.connection, run.messages, {
                            type: "read",
                            id: randomUUID(),
                            what,
                            agent: mayor,
                        }),
                    ).toMatchObject({ type: "read" });
                expect(
                    await request(run.connection, run.messages, {
                        type: "give",
                        id: randomUUID(),
                        to: mayor,
                        skill: { starter: "weather" },
                    }),
                ).toMatchObject({ kind: "notice" });
            })(),
            new Promise((_, reject) => {
                deadline = setTimeout(
                    () => reject(new Error("A skill paper blocked the server")),
                    10_000,
                );
            }),
        ]);
    } finally {
        clearTimeout(deadline);
        const release = await open(
            fifo,
            constants.O_RDWR | constants.O_NONBLOCK,
        );
        await rm(fifo);
        await release.writeFile("No blocked readers.");
        await release.close();
    }
}, 15_000);

it("refuses delivery after the agent replaces an accepted inbox with an outside link", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    const begin = await request(run.connection, run.messages, {
        type: "give",
        id: randomUUID(),
        to: mayor,
        files: [{ path: "folder/a.bin", size: 1 }],
    });
    if (begin.type !== "reply" || !begin.upload) throw new Error("No upload");
    const inbox = join(houseOf(run.home.layout, MAYOR), "inbox");
    await rename(inbox, `${inbox}-kept`);
    const outside = scratch();
    await symlink(outside, inbox);
    const reply = await request(run.connection, run.messages, {
        type: "give-part",
        id: randomUUID(),
        upload: begin.upload,
        path: "folder/a.bin",
        offset: 0,
        data: "QQ==",
    });
    expect(reply).toMatchObject({
        kind: "error",
        text: "The gift could not be delivered inside the inbox or skills folder. Nothing was handed over.",
    });
    expect(JSON.stringify(reply)).not.toContain(run.home.layout.dir);
    expect(await readdir(outside)).toEqual([]);
    expect(await readdir(`${inbox}-kept`)).toEqual([]);
    expect(await readdir(join(run.home.layout.dir, ".gifts"))).toEqual([]);
    expect(faux.state.callCount).toBe(0);
});

it.each(["inbox", "skills"])(
    "recreates a deleted %s folder for the next gift",
    async (shelf) => {
        const { models } = fauxModels();
        const run = await served({
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        const house = houseOf(run.home.layout, MAYOR);
        await rm(join(house, shelf), { recursive: true });
        const gift =
            shelf === "inbox"
                ? { files: [{ path: "a", data: "QQ==" }] }
                : { skill: { starter: "weather" } };
        expect(
            await request(run.connection, run.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                ...gift,
            }),
        ).toMatchObject({ kind: "notice" });
        expect(
            await readFile(
                join(
                    house,
                    shelf,
                    shelf === "inbox" ? "a" : "weather/SKILL.md",
                ),
                "utf8",
            ),
        ).toContain(shelf === "inbox" ? "A" : "weather");
    },
);

it("delivers files and skills without looking up a Python interpreter in the server's PATH", async () => {
    const { models } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    const bin = scratch();
    const marker = join(bin, "unsandboxed");
    await writeFile(
        join(bin, "python3"),
        `#!/bin/sh\nprintf ran > '${marker}'\nexit 1\n`,
        { mode: 0o755 },
    );
    const before = process.env.PATH;
    process.env.PATH = bin;
    try {
        for (const gift of [
            { files: [{ path: "a", data: "QQ==" }] },
            { skill: { starter: "weather" } },
        ])
            expect(
                await request(run.connection, run.messages, {
                    type: "give",
                    id: randomUUID(),
                    to: mayor,
                    ...gift,
                }),
            ).toMatchObject({ kind: "notice" });
        expect(await readdir(bin)).toEqual(["python3"]);
    } finally {
        process.env.PATH = before;
    }
});

it("rolls back earlier roots when a later delivery fails, with no server paths in the refusal", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    const inbox = join(houseOf(run.home.layout, MAYOR), "inbox");
    const long = "a".repeat(255);
    await writeFile(join(inbox, long), "Kept");
    const reply = await request(run.connection, run.messages, {
        type: "give",
        id: randomUUID(),
        to: mayor,
        files: [
            { path: "first", data: "QQ==" },
            { path: long, data: "Qg==" },
        ],
    });
    expect(reply).toMatchObject({
        kind: "error",
        text: expect.stringContaining("Nothing was handed over"),
    });
    expect(JSON.stringify(reply)).not.toContain(run.home.layout.dir);
    expect(await readdir(inbox)).toEqual([long]);
    expect(await readFile(join(inbox, long), "utf8")).toBe("Kept");
    expect(await readdir(join(run.home.layout.dir, ".gifts"))).toEqual([]);
    expect(faux.state.callCount).toBe(0);
});

it("records a skill giver's hamlet when it differs from the receiving agent's", async () => {
    const { models } = fauxModels();
    const run = await served({ models });
    const giver = await addHamlet(run.home, "other", "cartographer");
    const source = houseOf(run.home.layout, giver);
    await mkdir(join(source, "skills/maps"));
    await writeFile(join(source, "skills/maps/SKILL.md"), "Maps");
    expect(
        await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            skill: { from: { hamlet: "other", agent: "mayor" }, name: "maps" },
        }),
    ).toMatchObject({ kind: "notice" });
    expect(
        (
            await run.home.harness.snapshot(RecordDoc, "home", context)
        )?.decisions.at(-1),
    ).toMatchObject({ kind: "skill_given", from: "cartographer@other" });
});

it("refuses a manifest through a linked inbox before accepting any bytes", async () => {
    const { models } = fauxModels();
    const run = await served({ models });
    const inbox = join(houseOf(run.home.layout, MAYOR), "inbox");
    await rename(inbox, `${inbox}-kept`);
    const outside = scratch();
    await symlink(outside, inbox);
    expect(
        await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [{ path: "a", size: 1 }],
        }),
    ).toMatchObject({ kind: "error", text: expect.stringContaining("inbox") });
    expect(await readdir(outside)).toEqual([]);
});

it.each(["file", "skill"])(
    "hides filesystem failures for a %s gift",
    async (kind) => {
        const { models } = fauxModels();
        const run = await served({ models });
        const blocked =
            kind === "file"
                ? join(run.home.layout.dir, ".gifts")
                : join(houseOf(run.home.layout, MAYOR), "skills");
        await rm(blocked, { recursive: true, force: true });
        await writeFile(blocked, "Unavailable");
        const gift =
            kind === "file"
                ? { files: [{ path: "a", size: 1 }] }
                : { skill: { starter: "weather" } };
        const reply = await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            ...gift,
        });
        expect(reply).toMatchObject({
            kind: "error",
            text:
                kind === "file"
                    ? "The gift could not be staged. Nothing was handed over."
                    : "The gift could not be delivered inside the inbox or skills folder. Nothing was handed over.",
        });
        expect(JSON.stringify(reply)).not.toContain(run.home.layout.dir);
        expect(
            await readdir(join(houseOf(run.home.layout, MAYOR), "inbox")),
        ).toEqual([]);
    },
);

it("limits a connection to three pending gifts and frees its places on disconnect", async () => {
    const { models } = fauxModels();
    const run = await served({ models });
    const other = await joined(run.server.url, run.home.credentials.token);
    const manifest = (client: typeof other) =>
        request(client.connection, client.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [{ path: "a", size: 1 }],
        });
    for (let n = 0; n < 3; n++)
        expect(await manifest(other)).toMatchObject({
            kind: "notice",
            upload: expect.any(String),
        });
    expect(await manifest(other)).toMatchObject({
        kind: "error",
        text: "This connection already has three file gifts waiting. Finish one before starting another.",
    });
    expect(await readdir(join(run.home.layout.dir, ".gifts"))).toHaveLength(3);
    await other.connection.close();
    await expect
        .poll(async () => readdir(join(run.home.layout.dir, ".gifts")))
        .toEqual([]);
    expect(await manifest(run)).toMatchObject({
        kind: "notice",
        upload: expect.any(String),
    });
});

it("reserves no more than 32 MiB for an agent across every connection and releases discarded bytes", async () => {
    const { models } = fauxModels();
    const run = await served({ models });
    const other = await joined(run.server.url, run.home.credentials.token);
    const manifest = (client: typeof other, size: number) =>
        request(client.connection, client.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: [{ path: "a", size }],
        });
    for (const client of [run, other])
        expect(await manifest(client, 16 * 1024 * 1024)).toMatchObject({
            kind: "notice",
            upload: expect.any(String),
        });
    const refused = await manifest(run, 1);
    expect(refused).toMatchObject({
        kind: "error",
        text: "This agent already has 32 MB of file gifts waiting. Finish or discard one before starting another.",
    });
    expect(JSON.stringify(refused)).not.toContain(run.home.layout.dir);
    expect(await readdir(join(run.home.layout.dir, ".gifts"))).toHaveLength(2);
    await other.connection.close();
    await expect
        .poll(
            async () =>
                (await readdir(join(run.home.layout.dir, ".gifts"))).length,
        )
        .toBe(1);
    expect(await manifest(run, 16 * 1024 * 1024)).toMatchObject({
        kind: "notice",
        upload: expect.any(String),
    });
});

it("bounds even empty-file manifests before making staging entries", async () => {
    const { models } = fauxModels();
    const run = await served({ models });
    expect(
        await request(run.connection, run.messages, {
            type: "give",
            id: randomUUID(),
            to: mayor,
            files: Array.from({ length: 4097 }, (_, n) => ({
                path: `empty-${n}`,
                size: n === 4096 ? 1 : 0,
            })),
        }),
    ).toMatchObject({
        kind: "error",
        text: "A gift takes at most 4096 files at once.",
    });
    expect(
        await readdir(join(run.home.layout.dir, ".gifts")).catch(() => []),
    ).toEqual([]);
    expect(
        await readdir(join(houseOf(run.home.layout, MAYOR), "inbox")),
    ).toEqual([]);
});

it.each(["special", "large", "many"])(
    "refuses a %s skill tree specifically, while a valid sibling gift still arrives",
    async (kind) => {
        const { models } = fauxModels();
        const run = await served({ models });
        const source = await buildHouse(run.home.layout, {
            hamlet: "home",
            number: 1,
        });
        const folder = join(source, "skills/unsafe");
        await mkdir(folder);
        await writeFile(join(folder, "SKILL.md"), "---\nname: unsafe\n---\n");
        if (kind === "special") execFileSync("mkfifo", [join(folder, "pipe")]);
        else if (kind === "large") {
            const file = await open(join(folder, "sparse"), "w");
            await file.truncate(33 * 1024 * 1024);
            await file.close();
        } else
            for (let n = 0; n < 4095; n++)
                await mkdir(join(folder, `dir-${n}`));
        await run.home.harness.commit(async (tx) => {
            (await tx.doc(RecordDoc, "home", "home")).agents.push({
                number: 1,
                name: "scout",
                purpose: "Skills",
                model: null,
                since: 1,
            });
        }, context);
        expect(
            await request(run.connection, run.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                skill: {
                    from: { hamlet: "home", agent: "scout" },
                    name: "unsafe",
                },
            }),
        ).toMatchObject({
            kind: "error",
            text:
                kind === "special"
                    ? "A skill gift cannot include links or special files."
                    : kind === "large"
                      ? "The skill gift is larger than 32 MB."
                      : "A skill gift takes at most 4096 files and folders at once.",
        });
        expect(
            await readdir(join(houseOf(run.home.layout, MAYOR), "skills")),
        ).toEqual([]);
        expect(
            await request(run.connection, run.messages, {
                type: "give",
                id: randomUUID(),
                to: mayor,
                skill: { starter: "weather" },
            }),
        ).toMatchObject({
            kind: "notice",
            text: "Gave `mayor` the skill `weather`.",
        });
    },
    15_000,
);

it.each(
    ["link", "pipe", "folder", "shelf"].flatMap((kind) =>
        (["papers", "prompt"] as const).map((what) => ({ kind, what })),
    ),
)(
    "keeps $what responsive and private while a paper's $kind is swapped",
    async ({ kind, what }) => {
        const { models } = fauxModels();
        const run = await served({ models });
        const house = houseOf(run.home.layout, MAYOR);
        const folder = join(house, "skills/swapped");
        await mkdir(folder);
        const paper = join(folder, "SKILL.md");
        await writeFile(
            paper,
            "---\nname: swapped\ndescription: Original\n---\n",
        );
        const read = () =>
            request(run.connection, run.messages, {
                type: "read",
                id: randomUUID(),
                what,
                agent: mayor,
            });
        expect(JSON.stringify(await read())).toContain("Original");
        if (kind === "pipe") {
            await rm(paper);
            execFileSync("mkfifo", [paper]);
        } else {
            const root = scratch();
            const secret =
                kind === "shelf" ? join(root, "outside-secret-name") : root;
            await mkdir(secret, { recursive: true });
            const outside = join(secret, "SKILL.md");
            await writeFile(
                outside,
                "---\nname: outside-secret-name\ndescription: Never disclose this\n---\n",
            );
            await swappingDirectory(
                kind === "shelf"
                    ? join(house, "skills")
                    : kind === "folder"
                      ? folder
                      : paper,
                kind === "link" ? outside : root,
            );
        }
        for (let n = 0; n < 15; n++) {
            const reply = await read();
            expect(reply.type).toBe("read");
            expect(JSON.stringify(reply)).not.toContain("Never disclose this");
            expect(JSON.stringify(reply)).not.toContain("outside-secret-name");
        }
    },
    15_000,
);
