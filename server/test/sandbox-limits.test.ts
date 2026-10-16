import { execFileSync } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { describe, expect, it } from "vitest";
import { Bwrap } from "../src/sandbox/bwrap.ts";
import { type Policy, policyHash } from "../src/sandbox/policy.ts";
import { closedPolicy } from "../src/sandbox/reach.ts";
import { Seatbelt } from "../src/sandbox/seatbelt.ts";
import type { Sandbox } from "../src/sandbox.ts";
import { probeSandbox, SandboxEnv } from "../src/sandbox.ts";
import { context, houseEnv, scratch, sh } from "./support.ts";

describe("a sandboxed command's ending", () => {
    it("passes its exit code through, and a signal as 128 and its number", async () => {
        const { env } = houseEnv();

        expect(await sh(env, "echo out; echo err >&2; exit 3")).toEqual({
            exitCode: 3,
            output: "out\nerr\n",
        });
        expect((await sh(env, "kill -9 $$")).exitCode).toBe(137);
    });

    it("at its timeout kills the whole process group", async () => {
        const { house, env } = houseEnv();
        const late = join(house, "late");
        const started = Date.now();

        const ran = await env.exec(
            `(sleep 2; echo late > '${late}') & wait`,
            { timeout: 1 },
            context,
        );

        expect(ran.ok || ran.error.code).toBe("timeout");
        expect(Date.now() - started).toBeLessThan(2000);
        // Past the moment the process left behind would have written.
        await delay(2500 - (Date.now() - started));
        expect(existsSync(late)).toBe(false);
    });

    it("comes at the policy's wall time, even when asked to wait longer", async () => {
        const { env } = houseEnv({ wallSecs: 1 });
        const started = Date.now();

        const unasked = await env.exec("sleep 30", undefined, context);
        const longer = await env.exec("sleep 30", { timeout: 30 }, context);

        expect(unasked.ok || unasked.error.code).toBe("timeout");
        expect(longer.ok || longer.error.code).toBe("timeout");
        expect(Date.now() - started).toBeLessThan(6000);
    });
});

describe("a sandboxed command's limits", () => {
    it("are 60 s of CPU, 4096 processes, files up to 512 MB and no core dumps", async () => {
        const { env } = houseEnv();
        // A limit is only lowered, so a machine that allows fewer processes
        // keeps its own number, as macOS does.
        const machine = execFileSync("bash", [
            "-c",
            "ulimit -u hard; ulimit -u",
        ])
            .toString()
            .trim();
        const procs = machine === "unlimited" ? 4096 : Math.min(4096, +machine);

        // bash counts a file's size in blocks of 1024 bytes.
        expect(
            await sh(env, "ulimit -t; ulimit -u; ulimit -f; ulimit -c"),
        ).toEqual({ exitCode: 0, output: `60\n${procs}\n524288\n0\n` });
    });

    it("stop a file at the policy's size", async () => {
        const { house, env } = houseEnv({ maxFileMb: 1 });

        const ran = await sh(env, "head -c 3000000 /dev/zero > big");

        expect(ran.exitCode).not.toBe(0);
        expect(statSync(join(house, "big")).size).toBe(1024 * 1024);
    });

    it("stop a command at the policy's CPU time", async () => {
        const { env } = houseEnv({ cpuSecs: 1 });

        const ran = await sh(env, "while :; do :; done");

        // macOS sends SIGXCPU, which is 24. Linux sends SIGKILL, since the
        // soft limit is the hard one.
        expect([128 + 24, 128 + 9]).toContain(ran.exitCode);
    });

    it("are whole numbers of at least 1, or no environment is made", () => {
        for (const limits of [
            { cpuSecs: 1.5 },
            { maxProcs: -1 },
            { maxFileMb: Number.NaN },
            { wallSecs: 0 },
            { cpuSecs: 0 },
            { maxProcs: 0 },
            { maxFileMb: 0 },
        ]) {
            expect(() => houseEnv(limits)).toThrow(/is not a whole number/);
        }
    });

    it("hold a wall time up to the longest timer, 2147483 s, and no longer", async () => {
        const { env } = houseEnv({ wallSecs: 2_147_483 });

        expect(await sh(env, "echo hi")).toEqual({
            exitCode: 0,
            output: "hi\n",
        });
        expect(() => houseEnv({ wallSecs: 2_147_484 })).toThrow(
            "The limit wallSecs is more than 2147483 seconds: 2147484",
        );
    });
});

describe("a policy's hash", () => {
    const policy = (): Policy => ({
        ro: ["/shared"],
        rw: ["/house"],
        open: false,
        hidden: [],
        locked: [],
        net: false,
        services: [],
        env: { PATH: "/usr/bin:/bin", LANG: "C.UTF-8" },
    });

    it("is 12 hex characters, the same for equal policies", () => {
        const hash = policyHash(policy(), "bwrap");

        expect(hash).toMatch(/^[0-9a-f]{12}$/);
        expect(policyHash(policy(), "bwrap")).toBe(hash);
        expect(
            policyHash(
                {
                    ...policy(),
                    env: { LANG: "C.UTF-8", PATH: "/usr/bin:/bin" },
                    limits: { cpuSecs: 60, wallSecs: 120 },
                },
                "bwrap",
            ),
        ).toBe(hash);
    });

    it("changes for different reach or a different sandbox", () => {
        const hash = policyHash(policy(), "bwrap");
        expect(policyHash({ ...policy(), net: true }, "bwrap")).not.toBe(hash);
        expect(policyHash(policy(), "seatbelt")).not.toBe(hash);
    });

    const changes: { field: string; change: Partial<Policy> }[] = [
        { field: "ro", change: { ro: [] } },
        { field: "rw", change: { rw: ["/house", "/folder"] } },
        { field: "open", change: { open: true } },
        { field: "hidden", change: { hidden: ["/home"] } },
        { field: "locked", change: { locked: ["/house/.git"] } },
        { field: "fixed", change: { fixed: ["/house"] } },
        { field: "net", change: { net: true } },
        {
            field: "services",
            change: { services: [{ port: 9418, socket: "/git.sock" }] },
        },
        { field: "env", change: { env: { PATH: "/bin", LANG: "C.UTF-8" } } },
        { field: "cpuSecs", change: { limits: { cpuSecs: 61 } } },
        { field: "wallSecs", change: { limits: { wallSecs: 121 } } },
        { field: "maxProcs", change: { limits: { maxProcs: 4097 } } },
        { field: "maxFileMb", change: { limits: { maxFileMb: 513 } } },
    ];
    it.each(changes)(
        "isolates environments when $field changes",
        ({ change }) => {
            expect(policyHash({ ...policy(), ...change }, "bwrap")).not.toBe(
                policyHash(policy(), "bwrap"),
            );
        },
    );

    it("names the policy of each environment", () => {
        const house = scratch();
        const closed = closedPolicy(house);
        const shut = new SandboxEnv({ cwd: house, policy: closed });
        const networked = new SandboxEnv({
            cwd: house,
            policy: { ...closed, net: true },
        });

        expect(shut.policyHash).toMatch(/^[0-9a-f]{12}$/);
        expect(networked.policyHash).not.toBe(shut.policyHash);
    });
});

/** `sandbox` with its probe run as `argv` instead. */
function probing(sandbox: Sandbox, argv: string[]): Sandbox {
    return { ...sandbox, probe: { ...sandbox.probe, argv } };
}

describe("the sandbox probe", () => {
    const BWRAP_HINT =
        "bubblewrap is required for sandboxing. Install it (e.g. `apt install bubblewrap`). On Ubuntu 24.04+, AppArmor restricts unprivileged user namespaces; allow bwrap via an AppArmor profile or set kernel.apparmor_restrict_unprivileged_userns=0";

    it("passes on this machine", () => {
        expect(() => probeSandbox()).not.toThrow();
    });

    it("names what to install when bubblewrap is missing", () => {
        const [, ...args] = Bwrap.probe.argv;

        expect(() =>
            probeSandbox(probing(Bwrap, ["/no/such/bwrap", ...args])),
        ).toThrow(BWRAP_HINT);
    });

    it("says why bubblewrap failed when it cannot make its namespaces", () => {
        const failing = ["sh", "-c", "echo 'no namespaces' >&2; exit 1"];

        expect(() => probeSandbox(probing(Bwrap, failing))).toThrow(
            `bwrap probe failed (1): no namespaces. ${BWRAP_HINT}`,
        );
    });

    it("names what macOS needs when sandbox-exec is missing", () => {
        const [, ...args] = Seatbelt.probe.argv;

        expect(() =>
            probeSandbox(probing(Seatbelt, ["/no/such/sandbox-exec", ...args])),
        ).toThrow("macOS sandboxing needs /usr/bin/sandbox-exec");
    });
});
