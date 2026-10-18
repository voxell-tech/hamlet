import { randomUUID } from "node:crypto";
import { withAbortSignal } from "@earendil-works/chord/context";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import type { JsonObject } from "@earendil-works/pi-durable";
import { expect, it, vi } from "vitest";
import { say as chiefSay } from "../src/say.ts";
import { WorkspaceEnv } from "../src/workspace.ts";
import {
    context,
    fauxModels,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

// Live mode and explicit opt-in, dedicated disposable manager home/state only. Creates remote files and actors.
const url = process.env.HAMLET_WORKSPACE_GKE_URL;
const token = process.env.HAMLET_WORKSPACE_GKE_TOKEN;

it.skipIf(url === undefined || token === undefined)(
    "ordinary Harness tools on an approved real workspace deployment",
    async () => {
        if (url === undefined || token === undefined)
            throw new Error(
                "Set HAMLET_WORKSPACE_GKE_URL and HAMLET_WORKSPACE_GKE_TOKEN for a disposable manager",
            );
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: {
                HAMLET_WORKSPACE_URL: url,
                HAMLET_WORKSPACE_TOKEN: token,
                HAMLET_REFLECT_EVERY: "0",
            },
        });
        const configured = home.workspace;
        if (configured === null)
            throw new Error("Workspace connection was not configured");
        const prefix = `acceptance-${randomUUID()}`;
        const ordinary = async (name: string, args: JsonObject) => {
            faux.setResponses([
                fauxAssistantMessage(fauxToolCall(name, args), {
                    stopReason: "toolUse",
                }),
                fauxAssistantMessage("Done."),
            ]);
            expect(
                (
                    await (
                        await say(home, "Use the requested tool.")
                    ).wait(context)
                ).status,
            ).toBe("done");
            const last = (await transcript(await mayorChat(home)))
                .flatMap((entry) => entry.model ?? [])
                .filter((message) => message.role === "toolResult")
                .at(-1);
            if (last === undefined) throw new Error("No tool result");
            const text = last.content
                .flatMap((part) => (part.type === "text" ? [part.text] : []))
                .join("");
            expect(last.isError, text).not.toBe(true);
            return text;
        };
        const decree = (text: string) =>
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
        expect(await decree("/mode auto")).toMatchObject({ ok: true });
        expect(
            await ordinary("read", { path: "/library/system-info/SKILL.md" }),
        ).toContain("system-info");
        expect(
            await ordinary("bash", {
                command:
                    "python3 /library/system-info/scripts/info.py; if touch /library/forbidden; then exit 1; fi",
            }),
        ).toContain("OS: Linux");
        await ordinary("write", {
            path: `${prefix}.txt`,
            content: "first-17\n",
        });
        expect(
            await ordinary("bash", {
                command: `cat ${prefix}.txt; printf 'second-23\\n' >> ${prefix}.txt`,
            }),
        ).toContain("first-17");
        await ordinary("edit", {
            path: `${prefix}.txt`,
            edits: [{ oldText: "first-17", newText: "first-41" }],
        });
        expect(await ordinary("read", { path: `${prefix}.txt` })).toContain(
            "first-41\nsecond-23",
        );
        expect(await decree("/folder /project")).toMatchObject({ ok: true });
        await ordinary("write", {
            path: `${prefix}-shared`,
            content: "shared-37",
        });
        expect(
            await ordinary("search", { pattern: "shared-37", path: "." }),
        ).toContain(`${prefix}-shared`);
        const other = new WorkspaceEnv(configured, {
            workspace: "home",
            agent: 7,
            project: true,
            network: false,
        });
        expect(await other.readTextFile(`${prefix}-shared`, context)).toEqual({
            ok: true,
            value: "shared-37",
        });
        expect(
            (await other.readTextFile(`/house/${prefix}.txt`, context)).ok,
        ).toBe(false);
        const own = new WorkspaceEnv(configured, {
            workspace: "home",
            agent: 0,
            project: false,
            network: false,
        });
        const abort = new AbortController();
        const running = own.exec(
            `setsid bash -c 'while ! test -f ${prefix}-go; do sleep 0.05; done; echo escaped > ${prefix}-escaped' </dev/null >/dev/null 2>&1 & echo ready > ${prefix}-started; sleep 30`,
            undefined,
            withAbortSignal(abort.signal, context),
        );
        await vi.waitFor(
            async () =>
                expect(await own.exists(`${prefix}-started`, context)).toEqual({
                    ok: true,
                    value: true,
                }),
            { timeout: 20_000 },
        );
        abort.abort();
        expect(await running).toMatchObject({
            ok: false,
            error: { code: "aborted" },
        });
        expect(
            await own.writeFile(`${prefix}-go`, "go", context),
        ).toMatchObject({ ok: true });
        await new Promise((done) => setTimeout(done, 1000));
        expect(await own.exists(`${prefix}-escaped`, context)).toEqual({
            ok: true,
            value: false,
        });
        expect(
            await own.exec(
                "test ! -e /network.sock && test \"$(grep -Ec '^Cap(Inh|Prm|Eff|Bnd|Amb):[[:space:]]*0000000000000000$' /proc/self/status)\" = 5 && grep -q '^NoNewPrivs:[[:space:]]*1$' /proc/self/status",
                undefined,
                context,
            ),
        ).toMatchObject({ ok: true, value: { exitCode: 0 } });
        // Snapshot and cluster-network assertions require the separate approved operator matrix; no sleep implies proof.
    },
    180_000,
);
