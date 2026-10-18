import { execFile, spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { once } from "node:events";
import { existsSync } from "node:fs";
import { readFile, rm, symlink, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createServer as createGrpcServer } from "node:http2";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import { withAbortSignal } from "@earendil-works/chord/context";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import type { JsonObject } from "@earendil-works/pi-durable";
import { Type } from "typebox";
import { Value } from "typebox/value";
import { expect, it, vi } from "vitest";
import { WorkDoc } from "../src/environment.ts";
import { answer } from "../src/gate.ts";
import { houseOf } from "../src/home/layout.ts";
import { giftSkill } from "../src/house.ts";
import { PeersDoc } from "../src/letters.ts";
import { LibraryDoc } from "../src/library.ts";
import { LimitsDoc } from "../src/limits.ts";
import { RecordDoc, VotesDoc } from "../src/record.ts";
import { say as chiefSay } from "../src/say.ts";
import { edit as editForChief } from "../src/server/edits.ts";
import { read as readForChief } from "../src/server/reads.ts";
import { WorkspaceEnv } from "../src/workspace.ts";
import {
    closeAfter,
    context,
    fauxModels,
    mayorChat,
    openWaits,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

const binary = process.env.HAMLET_WORKSPACE_BIN;

it.skipIf(binary === undefined)(
    "ordinary tools share remote files through the real manager and confined worker",
    async () => {
        if (binary === undefined) throw new Error("Set HAMLET_WORKSPACE_BIN");
        const executable = resolve(binary);
        const root = scratch();
        const key = join(root, "manager.key");
        const generated = await promisify(execFile)(executable, [
            "keygen",
            key,
        ]);
        const publicKey = generated.stdout.trim();
        const actorUid = "11111111-1111-4111-8111-111111111111";
        const actorUidFile = join(root, "actor-uid");
        await writeFile(actorUidFile, actorUid);
        const { url: worker } = await service(executable, "worker", {
            listen: "127.0.0.1:0",
            root: join(root, "remote"),
            public_key: publicKey,
            actor_uid_file: actorUidFile,
        });
        const control = createGrpcServer();
        const actors = new Map<string, Buffer>();
        const lifecycle: string[] = [];
        const controlTokenFile = join(root, "control-token");
        let controlToken = "control-before-rotation";
        await writeFile(controlTokenFile, controlToken);
        control.on("stream", (stream, headers) => {
            const chunks: Buffer[] = [];
            stream.on("data", (chunk: Buffer) => chunks.push(chunk));
            stream.on("end", () => {
                if (headers.authorization !== `Bearer ${controlToken}`) {
                    stream.respond({
                        ":status": 200,
                        "content-type": "application/grpc",
                        "grpc-status": "16",
                    });
                    stream.end();
                    return;
                }
                const request = Buffer.concat(chunks).subarray(5);
                const object = field(request, 1);
                const verb = String(headers[":path"]).split("/").at(-1);
                if (
                    verb === undefined ||
                    ![
                        "GetActor",
                        "CreateActor",
                        "ResumeActor",
                        "SuspendActor",
                    ].includes(verb)
                )
                    throw new Error("Unexpected control RPC");
                lifecycle.push(verb);
                const creating =
                    headers[":path"] === "/ateapi.Control/CreateActor";
                const metadata = creating ? field(object, 1) : object;
                const name = field(metadata, 2).toString();
                const existing = actors.get(name);
                let actor =
                    existing ??
                    Buffer.concat([
                        bytes(
                            1,
                            Buffer.concat([
                                bytes(1, Buffer.from("trial")),
                                bytes(2, Buffer.from(name)),
                                bytes(
                                    3,
                                    Buffer.from(
                                        "11111111-1111-4111-8111-111111111111",
                                    ),
                                ),
                            ]),
                        ),
                        bytes(
                            4,
                            Buffer.concat([
                                bytes(1, Buffer.from("trial")),
                                bytes(2, Buffer.from("workspace")),
                            ]),
                        ),
                        bytes(7, Buffer.from([8, 4])),
                    ]);
                const status = creating
                    ? existing
                        ? "6"
                        : "0"
                    : existing
                      ? "0"
                      : "5";
                if (creating && !existing) actors.set(name, actor);
                if (
                    (verb === "ResumeActor" || verb === "SuspendActor") &&
                    existing
                ) {
                    actor = Buffer.concat([
                        actor,
                        bytes(
                            7,
                            Buffer.from([8, verb === "ResumeActor" ? 2 : 4]),
                        ),
                    ]);
                    actors.set(name, actor);
                }
                if (verb === "ResumeActor" || verb === "SuspendActor")
                    actor = bytes(1, actor);
                stream.respond(
                    { ":status": 200, "content-type": "application/grpc" },
                    { waitForTrailers: true },
                );
                stream.on("wantTrailers", () =>
                    stream.sendTrailers({ "grpc-status": status }),
                );
                const frame = Buffer.alloc(5);
                frame.writeUInt32BE(actor.length, 1);
                stream.end(
                    status === "0" ? Buffer.concat([frame, actor]) : undefined,
                );
            });
        });
        control.listen(0, "127.0.0.1");
        await once(control, "listening");
        closeAfter(
            () => new Promise<void>((done) => control.close(() => done())),
        );
        const controlAddress = control.address();
        if (controlAddress === null || typeof controlAddress === "string")
            throw new Error("Missing control address");
        let loseReply = false;
        const router = createServer(async (request, response) => {
            const target = request.headers["ate-target-actor"];
            if (
                typeof target !== "string" ||
                !target.startsWith("trial/") ||
                !actors.has(target.slice(6))
            ) {
                response.writeHead(404).end();
                return;
            }
            const chunks: Buffer[] = [];
            for await (const chunk of request) chunks.push(Buffer.from(chunk));
            const headers = new Headers();
            for (const [key, value] of Object.entries(request.headers))
                if (typeof value === "string" && key.startsWith("x-workspace-"))
                    headers.set(key, value);
            const routed = await fetch(`${worker}${request.url}`, {
                method: request.method ?? "POST",
                headers,
                body: Buffer.concat(chunks),
            });
            const body = Buffer.from(await routed.arrayBuffer());
            if (loseReply) {
                loseReply = false;
                response.destroy();
                return;
            }
            response.writeHead(routed.status, {
                "content-type": "application/json",
            });
            response.end(body);
        });
        router.listen(0, "127.0.0.1");
        await once(router, "listening");
        closeAfter(
            () => new Promise<void>((done) => router.close(() => done())),
        );
        const routeAddress = router.address();
        if (routeAddress === null || typeof routeAddress === "string")
            throw new Error("Missing router address");
        const configuration = {
            listen: "127.0.0.1:0",
            state: join(root, "state"),
            home: "offline-home",
            token: "offline-test-token",
            signing_key: key,
            control: `http://127.0.0.1:${controlAddress.port}`,
            control_token_file: controlTokenFile,
            router: `http://127.0.0.1:${routeAddress.port}`,
            atespace: "trial",
            template: "workspace",
        };
        const { url: manager, stop: stopManager } = await service(
            executable,
            "manager",
            configuration,
        );
        const connection = { url: manager, token: "offline-test-token" };
        const send = async (
            message: unknown,
            token = connection.token,
            url = manager,
        ) => {
            const response = await fetch(`${url}/v1`, {
                method: "POST",
                headers: { authorization: `Bearer ${token}` },
                body: JSON.stringify(message),
            });
            return response.json();
        };
        const fixture: unknown = JSON.parse(
            await readFile(
                new URL(
                    "../../crates/hamlet_workspace/fixtures/protocol-v1.json",
                    import.meta.url,
                ),
                "utf8",
            ),
        );
        const fixtureShape = Type.Object({
            cases: Type.Array(
                Type.Object({
                    name: Type.String(),
                    message: Type.Unknown(),
                    reply: Type.Unknown(),
                }),
            ),
        });
        if (!Value.Check(fixtureShape, fixture))
            throw new Error("Invalid shared protocol fixture");
        for (const example of fixture.cases)
            expect(await send(example.message), example.name).toEqual(
                example.reply,
            );
        const fixtureEnv = new WorkspaceEnv(connection, {
            workspace: "fixture",
            agent: 1,
            network: false,
            project: false,
        });
        expect(await fixtureEnv.readTextFile("greeting.txt", context)).toEqual({
            ok: true,
            value: "héllo, wörld ✓\n",
        });
        const binaryFile = await fixtureEnv.readBinaryFile("blob.bin", context);
        expect(
            binaryFile.ok && Buffer.from(binaryFile.value).toString("hex"),
        ).toBe("00fffe800a");
        controlToken = "control-after-rotation";
        expect(
            (await fixtureEnv.readTextFile("greeting.txt", context)).ok,
        ).toBe(false);
        await rm(controlTokenFile);
        expect(
            (await fixtureEnv.readTextFile("greeting.txt", context)).ok,
        ).toBe(false);
        await writeFile(controlTokenFile, `${controlToken}\n`);
        expect(await fixtureEnv.readTextFile("greeting.txt", context)).toEqual({
            ok: true,
            value: "héllo, wörld ✓\n",
        });
        const { models, faux } = fauxModels();
        const local = scratch();
        const home = await testHome(local, {
            models,
            env: {
                HAMLET_WORKSPACE: JSON.stringify({
                    url: manager,
                    token: "offline-test-token",
                }),
                // Automatic reflections would take the scripted model replies.
                HAMLET_REFLECT_EVERY: "0",
            },
        });
        const deed = async (text: string) =>
            chiefSay(
                home,
                {
                    type: "say",
                    id: randomUUID(),
                    to: { hamlet: "home", agent: "mayor" },
                    text,
                },
                context,
            );
        const call = async (name: string, args: JsonObject, failed = false) => {
            faux.setResponses([
                fauxAssistantMessage(fauxToolCall(name, args), {
                    stopReason: "toolUse",
                }),
                fauxAssistantMessage("Done."),
            ]);
            expect(
                (await (await say(home, "Do the work.")).wait(context)).status,
            ).toBe("done");
            const result = (await transcript(await mayorChat(home)))
                .flatMap((entry) => entry.model ?? [])
                .filter((message) => message.role === "toolResult")
                .at(-1);
            if (result === undefined) throw new Error("No tool result");
            const text = result.content
                .flatMap((part) => (part.type === "text" ? [part.text] : []))
                .join("");
            if (failed) expect(result.isError, text).toBe(true);
            else expect(result.isError, text).not.toBe(true);
            return text;
        };
        expect(
            await call("read", { path: "/library/system-info/SKILL.md" }),
        ).toContain("system-info");
        expect(
            await call("bash", {
                command: "python3 /library/system-info/scripts/info.py",
            }),
        ).toContain("OS: Linux");
        await call(
            "write",
            { path: "/library/system-info/SKILL.md", content: "wrong" },
            true,
        );
        expect(
            await call("bash", {
                command:
                    "if (echo wrong >/library/system-info/SKILL.md) 2>/dev/null; then exit 1; fi; echo readonly-library",
            }),
        ).toContain("readonly-library");
        const linked = join(home.layout.library, "private-link");
        await symlink(join(home.dir, "credentials.json"), linked);
        expect(
            await call("read", { path: "/library/private-link" }, true),
        ).toContain("shared library could not be read safely");
        await rm(linked);
        await call("write", { path: "receipt.txt", content: "first: 17\n" });
        expect(
            await call("bash", {
                command:
                    "cat receipt.txt; printf 'second: 23\\n' >> receipt.txt",
            }),
        ).toContain("first: 17");
        await call("edit", {
            path: "receipt.txt",
            edits: [{ oldText: "first: 17", newText: "first: 41" }],
        });
        expect(await call("read", { path: "receipt.txt" })).toContain(
            "first: 41\nsecond: 23",
        );
        expect(await call("find", { name: "receipt*" })).toContain(
            "receipt.txt",
        );
        expect(
            await call("search", { pattern: "second", path: "." }),
        ).toContain("second: 23");
        expect(
            existsSync(
                join(
                    houseOf(home.layout, { hamlet: "home", number: 0 }),
                    "receipt.txt",
                ),
            ),
        ).toBe(false);
        expect(
            (await (await mayorChat(home)).agent(context)).tools.map(
                (tool) => tool.name,
            ),
        ).not.toContain("sandbox");

        expect(await deed("/folder /tmp")).toMatchObject({
            ok: false,
            error: { kind: "folder-not-remote" },
        });
        expect(await deed("/folder /project")).toMatchObject({ ok: true });
        expect(
            (await home.harness.snapshot(WorkDoc, "home", context))?.folder,
        ).toBe("/project");
        // Ordinary path selection consults remote entries, even when the stored folder does not exist locally.
        await home.harness.commit(async (tx) => {
            Object.assign(await tx.doc(WorkDoc, "home", "home"), {
                folder: "/absent-on-server",
                mode: "auto",
            });
        }, context);
        await call("write", { path: "shared.txt", content: "shared-37" });
        expect(await call("read", { path: "receipt.txt" })).toContain(
            "first: 41",
        );
        expect(
            await call("bash", { command: "pwd; cat shared.txt" }),
        ).toContain("/project\nshared-37");
        const other = new WorkspaceEnv(connection, {
            workspace: "home",
            agent: 7,
            project: true,
            network: false,
        });
        expect(await other.readTextFile("shared.txt", context)).toEqual({
            ok: true,
            value: "shared-37",
        });
        expect(
            (await other.readTextFile("/house/receipt.txt", context)).ok,
        ).toBe(false);
        const own = new WorkspaceEnv(connection, {
            workspace: "home",
            agent: 0,
            project: false,
            network: false,
        });
        expect(
            (await own.readTextFile("/project/shared.txt", context)).ok,
        ).toBe(false);
        expect((await own.writeFile("/runner", "overwrite", context)).ok).toBe(
            false,
        );
        expect(
            (
                await own.exec(
                    "ln -s /project/shared.txt escape",
                    undefined,
                    context,
                )
            ).ok,
        ).toBe(true);
        expect((await own.readTextFile("escape", context)).ok).toBe(false);
        const privateRead = await own.exec(
            "cat /proc/1/fd/1 2>/dev/null",
            undefined,
            context,
        );
        expect(privateRead).toMatchObject({ ok: true, value: { exitCode: 1 } });
        const network = await own.exec(
            `echo unsafe >/dev/tcp/127.0.0.1/${routeAddress.port}`,
            undefined,
            context,
        );
        expect(network).toMatchObject({ ok: true, value: { exitCode: 1 } });
        const allowedNetwork = new WorkspaceEnv(connection, {
            workspace: "home",
            agent: 0,
            project: false,
            network: true,
        });
        let proxyOutput = "";
        expect(
            await allowedNetwork.exec(
                "curl -fsS --max-time 3 http://127.0.0.1/",
                {
                    onOutput: (text) => {
                        proxyOutput += text;
                    },
                },
                context,
            ),
        ).toMatchObject({ ok: true, value: { exitCode: 22 } });
        expect(proxyOutput).toContain("403");
        expect(
            await allowedNetwork.exec(
                "test -S /network.sock && test -s /etc/ssl/certs/ca-certificates.crt && test \"$(grep -Ec '^Cap(Inh|Prm|Eff|Bnd|Amb):[[:space:]]*0000000000000000$' /proc/self/status)\" = 5 && grep -q '^NoNewPrivs:[[:space:]]*1$' /proc/self/status",
                undefined,
                context,
            ),
        ).toMatchObject({ ok: true, value: { exitCode: 0 } });
        expect(
            await own.exec("test ! -e /network.sock", undefined, context),
        ).toMatchObject({ ok: true, value: { exitCode: 0 } });

        const aborted = new AbortController();
        const run = own.exec(
            "setsid bash -c 'sleep 0.6; echo escaped > escaped' </dev/null >/dev/null 2>&1 & echo ready > started; sleep 20",
            undefined,
            withAbortSignal(aborted.signal, context),
        );
        await vi.waitFor(async () =>
            expect(await own.exists("started", context)).toEqual({
                ok: true,
                value: true,
            }),
        );
        const sibling = other.exec(
            "sleep 0.2; echo survived > sibling",
            undefined,
            context,
        );
        aborted.abort();
        expect(await run).toMatchObject({
            ok: false,
            error: { code: "aborted" },
        });
        expect(await sibling).toMatchObject({
            ok: true,
            value: { exitCode: 0 },
        });
        expect(
            await own.exec(
                "setsid bash -c 'sleep 0.3; echo orphan > orphan' </dev/null >/dev/null 2>&1 &",
                undefined,
                context,
            ),
        ).toMatchObject({ ok: true, value: { exitCode: 0 } });
        expect(
            await own.exec("sleep 20", { timeout: 0.05 }, context),
        ).toMatchObject({ ok: false, error: { code: "timeout" } });
        expect(
            await own.exec("head -c 1100000 /dev/zero", undefined, context),
        ).toMatchObject({ ok: false });
        await new Promise((done) => setTimeout(done, 700));
        expect(await own.exists("escaped", context)).toEqual({
            ok: true,
            value: false,
        });
        expect(await own.exists("orphan", context)).toEqual({
            ok: true,
            value: false,
        });
        expect(await other.readTextFile("sibling", context)).toEqual({
            ok: true,
            value: "survived\n",
        });

        const id = randomUUID();
        const mutation = {
            kind: "execute",
            request: {
                version: 1,
                id,
                workspace: "home",
                agent: 0,
                project: false,
                network: false,
                operation: {
                    kind: "write",
                    path: "/house/once",
                    data: Buffer.from("once").toString("base64"),
                    append: true,
                },
            },
        };
        expect(await send(mutation)).toEqual({ kind: "ok", value: null });
        expect(await send(mutation)).toMatchObject({
            kind: "error",
            uncertain: true,
        });
        expect(await send({ kind: "status", id })).toEqual({
            kind: "ok",
            value: null,
        });
        expect(await own.readTextFile("once", context)).toEqual({
            ok: true,
            value: "once",
        });
        expect(await send(mutation, "incorrect-token")).toMatchObject({
            kind: "error",
            code: "permission_denied",
        });
        expect(
            await send({ home: "offline-home", message: mutation }, "", worker),
        ).toMatchObject({ kind: "error", code: "permission_denied" });

        await writeFile(actorUidFile, "22222222-2222-4222-8222-222222222222");
        expect(
            await send({
                ...mutation,
                request: { ...mutation.request, id: randomUUID() },
            }),
        ).toMatchObject({
            kind: "error",
            code: "permission_denied",
        });
        await writeFile(actorUidFile, actorUid);
        expect(await own.readTextFile("once", context)).toEqual({
            ok: true,
            value: "once",
        });

        const secret = "sk-workspace-test-secret-729";
        await home.credentials.modify("openai", async () => ({
            type: "api_key",
            key: secret,
        }));
        expect(
            await call("bash", {
                command: "printf 'sk-workspace-'; printf 'test-secret-729'",
            }),
        ).toContain("[REDACTED]");
        const spilled = await call("bash", {
            command:
                "for i in $(seq 1 2100); do printf 'sk-workspace-'; printf 'test-secret-729\\n'; done",
        });
        expect(spilled).not.toContain(secret);
        expect(
            JSON.stringify(await transcript(await mayorChat(home))),
        ).not.toContain(secret);

        // House files are authoritative remotely: papers, prompts and chief edits use the workspace's /house.
        const mayor = { hamlet: "home", agent: "mayor" };
        const ask = (what: "papers" | "prompt") =>
            readForChief(
                home,
                { type: "read", id: randomUUID(), what, agent: mayor },
                context,
            );
        expect(home.houses.of({ hamlet: "home", number: 0 }).path).toBe(
            "/house",
        );
        expect(await own.readTextFile("/house/NOTES.md", context)).toEqual({
            ok: true,
            value: "# Notes\n",
        });
        await call("write", {
            path: "/house/NOTES.md",
            content: "# Notes\nremote-note-53\n",
        });
        expect(
            await own.exec(
                "mkdir skills/tally skills/linked && ln -s ../../NOTES.md skills/linked/SKILL.md",
                undefined,
                context,
            ),
        ).toMatchObject({ ok: true, value: { exitCode: 0 } });
        await call("write", {
            path: "/house/skills/tally/SKILL.md",
            content:
                "---\nname: tally\ndescription: Counts remote receipts.\nusage: ./skills/tally/run\n---\n",
        });
        expect(
            await own.exec(
                "chmod 750 skills/tally; chmod 640 skills/tally/SKILL.md",
                undefined,
                context,
            ),
        ).toMatchObject({ ok: true, value: { exitCode: 0 } });
        expect(await ask("papers")).toMatchObject({
            ok: true,
            value: {
                papers: {
                    notes: "# Notes\nremote-note-53",
                    skills: [
                        {
                            name: "tally",
                            description: "Counts remote receipts.",
                        },
                    ],
                },
            },
        });
        const prompt = await ask("prompt");
        if (!prompt.ok || !("prompt" in prompt.value))
            throw new Error("No prompt");
        expect(prompt.value.prompt).toContain("runs in /project");
        expect(prompt.value.prompt).not.toContain("/absent-on-server");
        expect(prompt.value.prompt).toContain(
            "# NOTES.md\n# Notes\nremote-note-53",
        );
        expect(prompt.value.prompt).toContain(
            "- tally: Counts remote receipts. Run: `cd '/house' && ./skills/tally/run` (skills/tally/SKILL.md)",
        );
        expect(prompt.value.prompt).not.toContain("linked");
        expect(prompt.value.prompt).toContain(
            "# Library\nSkills every agent shares, kept in /library",
        );
        expect(prompt.value.prompt).not.toContain(home.layout.library);
        const reader = new WorkspaceEnv(connection, {
            workspace: "home",
            agent: 0,
            project: false,
            network: false,
            read_only: true,
        });
        expect(
            (
                await reader.openBinaryReader(
                    "/house/skills/linked/SKILL.md",
                    { noFollow: true },
                    context,
                )
            ).ok,
        ).toBe(false);
        expect(
            (await reader.writeFile("/house/NOTES.md", "lost", context)).ok,
        ).toBe(false);
        expect(
            await reader.exec(
                "echo lost > /house/NOTES.md",
                undefined,
                context,
            ),
        ).toMatchObject({ ok: true, value: { exitCode: 1 } });
        expect(await reader.readTextFile("/house/NOTES.md", context)).toEqual({
            ok: true,
            value: "# Notes\nremote-note-53\n",
        });
        expect(
            await editForChief(
                home,
                {
                    type: "edit",
                    id: randomUUID(),
                    doc: "notes",
                    agent: mayor,
                    text: "# Notes\nchief-note-61\n",
                },
                context,
            ),
        ).toEqual({ ok: true, value: "Saved." });
        expect(await call("read", { path: "/house/NOTES.md" })).toContain(
            "chief-note-61",
        );

        // A manual-mode write previews the remote before-image and still waits for the chief, even in the house.
        await home.harness.commit(async (tx) => {
            Object.assign(await tx.doc(WorkDoc, "home", "home"), {
                folder: "/absent-on-server",
                mode: "manual",
            });
        }, context);
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("write", {
                    path: "/house/NOTES.md",
                    content: "# Notes\napproved-note-67\n",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Done."),
        ]);
        const held = await say(home, "Update the notes.");
        await vi.waitFor(async () =>
            expect(await openWaits(home)).toHaveLength(1),
        );
        const wait = (await openWaits(home))[0];
        if (wait === undefined) throw new Error("No wait");
        expect(wait.danger).toBe("edit");
        expect(wait.about).toBe(
            "/house/NOTES.md\nReplace:\n# Notes\nchief-note-61\n\nWith:\n# Notes\napproved-note-67\n",
        );
        expect(wait.change).toMatchObject({ created: false });
        expect(await own.readTextFile("/house/NOTES.md", context)).toEqual({
            ok: true,
            value: "# Notes\nchief-note-61\n",
        });
        await answer(
            home,
            { wait: wait.id, chief: home.chief, choice: "allow" },
            context,
        );
        expect((await held.wait(context)).status).toBe("done");
        expect(await own.readTextFile("/house/NOTES.md", context)).toEqual({
            ok: true,
            value: "# Notes\napproved-note-67\n",
        });
        expect(
            await readFile(
                join(
                    root,
                    "remote",
                    "offline-home",
                    "home",
                    "houses",
                    "0",
                    "receipt.txt",
                ),
                "utf8",
            ),
        ).toBe("first: 41\nsecond: 23\n");
        await home.harness.commit(async (tx) => {
            (await tx.doc(WorkDoc, "home", "home")).mode = "auto";
        }, context);
        await call("create_agent", {
            name: "keeper",
            purpose: "Keep files",
            reason: "Test workspace lifecycle",
        });
        await call("create_agent", {
            name: "scout",
            purpose: "Find files",
            reason: "Test workspace lifecycle",
        });
        const record = await home.harness.snapshot(RecordDoc, "home", context);
        const keeper = record?.agents.find((agent) => agent.name === "keeper");
        const scout = record?.agents.find((agent) => agent.name === "scout");
        if (!keeper || !scout) throw new Error("Agents not created");
        const keeperEnv = new WorkspaceEnv(connection, {
            workspace: "home",
            agent: keeper.number,
            network: false,
            project: false,
        });
        const scoutEnv = new WorkspaceEnv(connection, {
            workspace: "home",
            agent: scout.number,
            network: false,
            project: false,
        });
        await call("bash", { command: "mkdir /house/skills/shared-value" });
        await call("write", {
            path: "/house/skills/shared-value/SKILL.md",
            content:
                "---\nname: shared-value\ndescription: Prints the shared receipt.\nusage: bash skills/shared-value/run.sh\n---\nRun the script.",
        });
        await call("write", {
            path: "/house/skills/shared-value/run.sh",
            content: "printf 'shared-131\\n'\n",
        });
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("submit_skill", {
                    skill: "shared-value",
                    reason: "Every agent can use this.",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("ACCEPT. It is reusable."),
            fauxAssistantMessage("Shared."),
        ]);
        expect(
            (await (await say(home, "Share the skill.")).wait(context)).status,
        ).toBe("done");
        expect(
            (await home.harness.snapshot(LibraryDoc, context))?.events,
        ).toContainEqual(
            expect.objectContaining({ kind: "shelved", skill: "shared-value" }),
        );
        expect(await own.exists("skills/shared-value", context)).toEqual({
            ok: true,
            value: false,
        });
        expect(
            await call("bash", {
                command: "bash /library/shared-value/run.sh",
            }),
        ).toContain("shared-131");
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("ask_agent", {
                    name: "scout",
                    message: "Run the shared library's shared-value skill.",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage(
                fauxToolCall("bash", {
                    command: "bash /library/shared-value/run.sh",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("The library says shared-131."),
            fauxAssistantMessage("Received."),
        ]);
        expect(
            (await (await say(home, "Ask scout.")).wait(context)).status,
        ).toBe("done");
        const peer = (
            await home.harness.snapshot(
                PeersDoc,
                `home:${scout.number}`,
                context,
            )
        )?.questions["home:0"];
        if (peer === undefined) throw new Error("No question conversation");
        expect(JSON.stringify(await transcript(peer, home.harness))).toContain(
            "shared-131",
        );
        expect(
            JSON.stringify(await transcript(await mayorChat(home))),
        ).toContain("The library says shared-131.");
        expect(await deed("/unshelve shared-value Finished")).toMatchObject({
            ok: true,
        });
        await call("read", { path: "/library/shared-value/run.sh" }, true);

        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("object", { reason: "I need confirmation." }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage(
                fauxToolCall("write", {
                    path: "/house/objection-result",
                    content: "approved-137",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Done."),
        ]);
        const objected = await say(home, "Write only after confirmation.");
        await vi.waitFor(async () =>
            expect(await openWaits(home)).toHaveLength(1),
        );
        const objection = (await openWaits(home))[0];
        if (objection === undefined) throw new Error("No objection");
        expect(objection.kind).toBe("objection");
        expect(await own.exists("objection-result", context)).toEqual({
            ok: true,
            value: false,
        });
        await answer(
            home,
            { wait: objection.id, chief: home.chief, choice: "go" },
            context,
        );
        expect((await objected.wait(context)).status).toBe("done");
        expect(await own.readTextFile("objection-result", context)).toEqual({
            ok: true,
            value: "approved-137",
        });
        await call(
            "call_vote",
            {
                kind: "found_hamlet",
                hamlet: "elsewhere",
                mayor: "scout",
                purpose: "Trial",
                reason: "Trial",
            },
            true,
        );
        expect(
            (await home.harness.snapshot(VotesDoc, "home", context))?.votes ??
                [],
        ).toHaveLength(0);
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, "home", "home")).votes.leastVoters = 2;
        }, context);
        faux.setResponses([
            fauxAssistantMessage("The mayor has answered."),
            fauxAssistantMessage(
                fauxToolCall("cast_ballot", {
                    choice: "abstain",
                    reason: "",
                    evidence: "",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage(
                fauxToolCall("cast_ballot", {
                    choice: "abstain",
                    reason: "",
                    evidence: "",
                }),
                { stopReason: "toolUse" },
            ),
        ]);
        expect(await deed("/vote scout A trial")).toMatchObject({ ok: true });
        await vi.waitFor(
            async () =>
                expect(
                    (
                        await home.harness.snapshot(VotesDoc, "home", context)
                    )?.votes.at(-1)?.standing,
                ).toMatchObject({ failed: expect.any(String) }),
            { timeout: 10000 },
        );
        expect(
            (await home.harness.snapshot(VotesDoc, "home", context))?.votes.at(
                -1,
            )?.ballots,
        ).toHaveLength(2);
        expect(await deed("/votes")).toMatchObject({ ok: true });
        expect(
            await giftSkill(
                home,
                { hamlet: "home", agent: "scout" },
                "tally",
                mayor,
                context,
            ),
        ).toMatchObject({ ok: true });
        expect(
            await scoutEnv.readTextFile("skills/tally/SKILL.md", context),
        ).toMatchObject({ ok: true });
        expect(
            await scoutEnv.tree(
                "skills/tally",
                { entries: 10, bytes: 4096 },
                context,
            ),
        ).toMatchObject({
            ok: true,
            value: [
                { path: ".", kind: "directory", mode: 0o750 },
                { path: "SKILL.md", kind: "file", mode: 0o640 },
            ],
        });
        expect(
            await scoutEnv.writeFile("found.txt", "found-83", context),
        ).toMatchObject({ ok: true });
        expect(
            await scoutEnv.writeFile("NOTES.md", "scout-note-89", context),
        ).toMatchObject({ ok: true });
        expect(
            await keeperEnv.writeFile("NOTES.md", "keeper-note-97", context),
        ).toMatchObject({ ok: true });
        expect(
            await deed("/merge scout into keeper Shared work"),
        ).toMatchObject({
            ok: true,
            value: { text: expect.stringContaining("Merged") },
        });
        expect(await keeperEnv.readTextFile("found.txt", context)).toEqual({
            ok: true,
            value: "found-83",
        });
        expect(await scoutEnv.readTextFile("found.txt", context)).toEqual({
            ok: true,
            value: "found-83",
        });
        expect(await deed("/restore scout")).toMatchObject({ ok: true });
        expect(await keeperEnv.exists("found.txt", context)).toEqual({
            ok: true,
            value: false,
        });
        expect(await keeperEnv.readTextFile("NOTES.md", context)).toEqual({
            ok: true,
            value: "keeper-note-97",
        });
        expect(await deed("/remove scout Finished")).toMatchObject({
            ok: true,
        });
        expect(await deed("/restore scout")).toMatchObject({ ok: true });
        expect(await scoutEnv.readTextFile("found.txt", context)).toEqual({
            ok: true,
            value: "found-83",
        });
        expect(
            existsSync(
                join(
                    houseOf(home.layout, {
                        hamlet: "home",
                        number: scout.number,
                    }),
                    "found.txt",
                ),
            ),
        ).toBe(false);
        expect(actors.size).toBe(2);
        expect(lifecycle.filter((verb) => verb === "CreateActor")).toHaveLength(
            2,
        );
        expect(lifecycle).toContain("ResumeActor");
        // Lose a real append's reply, then restart only the manager. Recovery must query, never execute again.
        const interrupted = {
            ...mutation,
            request: {
                ...mutation.request,
                id: randomUUID(),
                operation: {
                    ...mutation.request.operation,
                    path: "/house/recovered",
                },
            },
        };
        loseReply = true;
        expect(await send(interrupted)).toMatchObject({
            kind: "error",
            uncertain: true,
        });
        await stopManager();
        await service(executable, "manager", {
            ...configuration,
            listen: new URL(manager).host,
        });
        await vi.waitFor(async () =>
            expect(
                await send({ kind: "status", id: interrupted.request.id }),
            ).toEqual({
                kind: "ok",
                value: null,
            }),
        );
        expect(await send(interrupted)).toMatchObject({
            kind: "error",
            uncertain: true,
        });
        expect(await own.readTextFile("recovered", context)).toEqual({
            ok: true,
            value: "once",
        });
        const cancelled = fixture.cases.find(
            (example) => example.name === "cancelled_execute",
        );
        if (cancelled === undefined)
            throw new Error("Missing cancellation fixture");
        expect(await send(cancelled.message)).toEqual(cancelled.reply);
        expect(await fixtureEnv.exists("cancelled-escape", context)).toEqual({
            ok: true,
            value: false,
        });
        expect(lifecycle.filter((verb) => verb === "CreateActor")).toHaveLength(
            2,
        );
        // A vanished bound actor must not be replaced with a fresh empty actor.
        actors.clear();
        expect((await own.readTextFile("receipt.txt", context)).ok).toBe(false);
        expect(actors.size).toBe(0);
    },
    60_000,
);

async function service(
    executable: string,
    mode: string,
    configuration: object,
): Promise<{ url: string; stop(): Promise<void> }> {
    const config = join(scratch(), `${mode}.json`);
    await writeFile(config, JSON.stringify(configuration), { mode: 0o600 });
    const child = spawn(executable, [mode, config], {
        stdio: ["ignore", "pipe", "pipe"],
    });
    const stop = async () => {
        if (child.exitCode !== null || child.signalCode !== null) return;
        child.kill("SIGTERM");
        await once(child, "exit");
    };
    closeAfter(stop);
    let diagnostic = "";
    child.stderr.on("data", (chunk: Buffer) => {
        diagnostic += chunk.toString();
    });
    const ready = new Promise<string>((accept, reject) => {
        let output = "";
        child.on("error", reject);
        child.on("exit", () =>
            reject(new Error(`${mode} exited: ${diagnostic}`)),
        );
        child.stdout.on("data", (chunk: Buffer) => {
            output += chunk.toString();
            const match = output.match(/LISTEN (http:\/\/[^\s]+)/);
            if (match?.[1]) accept(match[1]);
        });
    });
    return { url: await ready, stop };
}

function varint(number: number): Buffer {
    const bytes: number[] = [];
    do {
        bytes.push((number & 127) | (number > 127 ? 128 : 0));
        number >>>= 7;
    } while (number > 0);
    return Buffer.from(bytes);
}

function bytes(tag: number, value: Buffer): Buffer {
    return Buffer.concat([varint(tag * 8 + 2), varint(value.length), value]);
}

function field(message: Buffer, wanted: number): Buffer {
    let at = 0;
    const integer = () => {
        let value = 0;
        for (let shift = 0; shift < 35; shift += 7) {
            const byte = message[at++];
            if (byte === undefined)
                throw new Error("Invalid protobuf fixture input");
            value |= (byte & 127) << shift;
            if (byte < 128) return value;
        }
        throw new Error("Oversize protobuf fixture input");
    };
    while (at < message.length) {
        const key = integer();
        if ((key & 7) === 0) {
            integer();
            continue;
        }
        if ((key & 7) !== 2) throw new Error("Unsupported fixture wire type");
        const length = integer();
        const value = message.subarray(at, at + length);
        at += length;
        if (key >>> 3 === wanted) return value;
    }
    return Buffer.alloc(0);
}
