import { randomUUID } from "node:crypto";
import {
    lstat,
    mkdir,
    readdir,
    readFile,
    rename,
    rm,
    symlink,
    writeFile,
} from "node:fs/promises";
import { createServer } from "node:net";
import { dirname, join } from "node:path";
import { getOrThrow } from "@earendil-works/pi-durable/env";
import { expect, it, vi } from "vitest";
import { SandboxEnv } from "../src/sandbox/env.ts";
import { closedPolicy, openPolicy } from "../src/sandbox/reach.ts";
import { context, scratch, sh } from "./support.ts";

const kinds = [
    "house",
    "folder",
    "hidden home",
    "manifest write",
    "tool folder",
    "tree source",
    "stage",
    "private temp",
    "output",
] as const;
type Kind = (typeof kinds)[number];

async function setup(kind: Kind) {
    const house = scratch(),
        root = scratch();
    const policy =
        kind === "folder"
            ? openPolicy({ house, folder: root, home: scratch() })
            : kind === "hidden home"
              ? openPolicy({ house, folder: scratch(), home: root })
              : closedPolicy(house);
    if (kind === "manifest write" || kind === "stage") policy.rw.push(root);
    if (kind === "tool folder" || kind === "tree source") policy.ro.push(root);
    if (kind === "tool folder") policy.locked.push(root);
    const env = new SandboxEnv({ cwd: house, policy, tmpScope: randomUUID() });
    const tmp = (await sh(env, 'printf %s "$TMPDIR"')).output;
    const path =
        kind === "house"
            ? house
            : kind === "private temp"
              ? tmp
              : kind === "output"
                ? dirname(
                      getOrThrow(await env.createTempFile(undefined, context)),
                  )
                : root;
    await writeFile(join(path, "secret"), "ordinary");
    if (kind === "hidden home")
        expect((await sh(env, `cat '${path}/secret'`)).exitCode).not.toBe(0);
    else
        expect((await sh(env, `cat '${path}/secret'`)).output).toBe("ordinary");
    return { env, path };
}

it.each(kinds)(
    "keeps the %s root itself fixed while ordinary contents stay usable",
    async (kind) => {
        const { env, path } = await setup(kind);
        const outside = scratch();
        await writeFile(join(outside, "secret"), "chief-credentials");
        try {
            const attack = await sh(
                env,
                `rm -rf -- '${path}' && ln -s '${outside}' '${path}'`,
            );
            expect(attack.exitCode).not.toBe(0);
            expect((await lstat(path)).isDirectory()).toBe(true);
            const next = await sh(
                env,
                `cat '${path}/secret'; echo changed > '${path}/secret'`,
            );
            expect(next.output).not.toContain("chief-credentials");
            expect(await readFile(join(outside, "secret"), "utf8")).toBe(
                "chief-credentials",
            );
            if (!["hidden home", "tool folder", "tree source"].includes(kind))
                expect(
                    (
                        await sh(
                            env,
                            `echo ordinary > '${path}/note'; cat '${path}/note'`,
                        )
                    ).output,
                ).toBe("ordinary\n");
            expect(
                (await env.remove(path, { recursive: true }, context)).ok,
            ).toBe(false);
            expect((await env.createTempFile(undefined, context)).ok).toBe(
                true,
            );
        } finally {
            await env.cleanup(context);
        }
    },
);

it.each(kinds)(
    "refuses a moved %s root replaced by another real directory before commands and file calls",
    async (kind) => {
        const { env, path } = await setup(kind);
        const old = `${path}-moved`;
        await rename(path, old);
        await mkdir(path);
        await writeFile(join(path, "secret"), "replacement");
        try {
            const read = await env.exec(
                `cat '${path}/secret'; echo changed > '${path}/secret'`,
                undefined,
                context,
            );
            expect.soft(read.ok).toBe(false);
            expect
                .soft(
                    (await env.readTextFile(join(path, "secret"), context)).ok,
                )
                .toBe(false);
            expect
                .soft((await env.createTempFile(undefined, context)).ok)
                .toBe(false);
            expect
                .soft((await env.watch([], () => {}, context)).ok)
                .toBe(false);
            expect(await readFile(join(path, "secret"), "utf8")).toBe(
                "replacement",
            );
        } finally {
            await rm(path, { recursive: true });
            await rename(old, path);
            await env.cleanup(context);
        }
    },
);

it.each(kinds)(
    "refuses a linked %s root before commands and file calls",
    async (kind) => {
        const { env, path } = await setup(kind);
        const outside = scratch(),
            old = `${path}-moved`;
        await writeFile(join(outside, "secret"), "chief-credentials");
        await rename(path, old);
        await symlink(outside, path);
        try {
            const read = await env.exec(
                `cat '${path}/secret'; echo changed > '${path}/secret'`,
                undefined,
                context,
            );
            expect.soft(read.ok).toBe(false);
            expect
                .soft(
                    (await env.readTextFile(join(path, "secret"), context)).ok,
                )
                .toBe(false);
            expect
                .soft((await env.createTempFile(undefined, context)).ok)
                .toBe(false);
            expect
                .soft((await env.watch([], () => {}, context)).ok)
                .toBe(false);
            expect(await readFile(join(outside, "secret"), "utf8")).toBe(
                "chief-credentials",
            );
        } finally {
            await rm(path);
            await rename(old, path);
            await env.cleanup(context);
        }
    },
);

it("keeps the temp container fixed under a broad write grant and refuses a planted container link", async () => {
    const base = scratch(),
        outside = scratch();
    vi.stubEnv("TMPDIR", base);
    let env: SandboxEnv | undefined;
    let container = "";
    try {
        const policy = closedPolicy(base);
        env = new SandboxEnv({ cwd: base, policy, tmpScope: "container" });
        container = dirname((await sh(env, 'printf %s "$TMPDIR"')).output);
        expect(
            (await sh(env, `mv '${container}' '${container}-moved'`)).exitCode,
        ).not.toBe(0);
        expect((await lstat(container)).isDirectory()).toBe(true);
        await env.cleanup(context);
        env = undefined;
        await rename(container, `${container}-old`);
        await symlink(outside, container);
        expect(
            () => new SandboxEnv({ cwd: base, policy, tmpScope: "container" }),
        ).toThrow(/root|directory/i);
        expect(await readdir(outside)).toEqual([]);
    } finally {
        if (
            container &&
            (await lstat(container).catch(() => undefined))?.isSymbolicLink()
        ) {
            await rm(container);
            await rename(`${container}-old`, container);
        } else if (
            container &&
            (await lstat(`${container}-moved`).catch(() => undefined))
        ) {
            await rename(`${container}-moved`, container);
        }
        await env?.cleanup(context);
        vi.unstubAllEnvs();
    }
});

it("refuses a service bind source redirected from its socket to an outside file", async () => {
    const house = scratch(),
        socket = join(scratch(), "service.sock"),
        outside = join(scratch(), "secret");
    await writeFile(outside, "chief-credentials");
    const listener = createServer();
    await new Promise<void>((done) => listener.listen(socket, done));
    const policy = closedPolicy(house);
    policy.services.push({ socket, port: 32123 });
    const env = new SandboxEnv({ cwd: house, policy, tmpScope: randomUUID() });
    await rename(socket, `${socket}-old`);
    await symlink(outside, socket);
    try {
        const result = await env.exec(
            `cat '${socket}'; echo changed > '${socket}'`,
            undefined,
            context,
        );
        expect.soft(result.ok).toBe(false);
        expect
            .soft((await env.createTempFile(undefined, context)).ok)
            .toBe(false);
        expect(await readFile(outside, "utf8")).toBe("chief-credentials");
    } finally {
        await rm(socket);
        await rename(`${socket}-old`, socket);
        await env.cleanup(context);
        await new Promise<void>((done) => listener.close(() => done()));
    }
});

it("keeps complete command output readable in its fixed spill folder", async () => {
    const { env } = await setup("output");
    try {
        const result = getOrThrow(
            await env.exec(
                "printf ordinary-output",
                { spill: { afterBytes: 1, afterLines: 1 } },
                context,
            ),
        );
        expect(result.spillPath).toBeDefined();
        expect(
            getOrThrow(await env.readTextFile(result.spillPath ?? "", context)),
        ).toBe("ordinary-output");
    } finally {
        await env.cleanup(context);
    }
});

it("never spills through a replaced output file outside the sandbox", async () => {
    const { env } = await setup("house");
    const outside = join(scratch(), "secret");
    await writeFile(outside, "chief-credentials");
    const create = env.createTempFile.bind(env);
    const allocating = vi
        .spyOn(env, "createTempFile")
        .mockImplementation(async (options, c) => {
            const file = await create(options, c);
            if (file.ok)
                await sh(
                    env,
                    `rm -- '${file.value}'; ln -s '${outside}' '${file.value}'`,
                );
            return file;
        });
    try {
        await env
            .exec(
                "printf agent-output",
                { spill: { afterBytes: 1, afterLines: 1 } },
                context,
            )
            .catch(() => undefined);
        expect(await readFile(outside, "utf8")).toBe("chief-credentials");
    } finally {
        allocating.mockRestore();
        await env.cleanup(context);
    }
});

it("keeps writable ancestors from carrying a nested root away", async () => {
    const broad = scratch(),
        nested = join(broad, "parent/root");
    await mkdir(nested, { recursive: true });
    const policy = closedPolicy(broad);
    policy.rw.push(nested);
    const env = new SandboxEnv({ cwd: broad, policy, tmpScope: randomUUID() });
    try {
        expect(
            (
                await sh(
                    env,
                    `mv '${join(broad, "parent")}' '${join(broad, "moved")}'`,
                )
            ).exitCode,
        ).not.toBe(0);
        expect((await lstat(nested)).isDirectory()).toBe(true);
        expect(
            (
                await sh(
                    env,
                    `echo ordinary > '${nested}/note'; cat '${nested}/note'`,
                )
            ).output,
        ).toBe("ordinary\n");
        expect(
            (
                await env.renameFile(
                    join(broad, "parent"),
                    join(broad, "moved"),
                    context,
                )
            ).ok,
        ).toBe(false);
    } finally {
        await env.cleanup(context);
    }
});

it("keeps output folders fixed while allowing ordinary output files", async () => {
    const { env, path } = await setup("output");
    const outside = scratch();
    await writeFile(join(outside, "secret"), "chief-credentials");
    try {
        await sh(env, `rm -rf -- '${path}' && ln -s '${outside}' '${path}'`);
        expect((await lstat(path)).isDirectory()).toBe(true);
        expect(
            (
                await sh(
                    env,
                    `echo ordinary > '${path}/note'; cat '${path}/note'`,
                )
            ).output,
        ).toBe("ordinary\n");
        expect((await env.remove(path, { recursive: true }, context)).ok).toBe(
            false,
        );
        expect(await readFile(join(outside, "secret"), "utf8")).toBe(
            "chief-credentials",
        );
    } finally {
        if ((await lstat(path)).isSymbolicLink()) {
            await rm(path);
            await mkdir(path);
        }
        await env.cleanup(context);
    }
});
