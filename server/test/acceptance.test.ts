import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, it, vi } from "vitest";
import { connectChief, reachHome } from "../src/cli/client.ts";
import { hamlet, server } from "./cli-support.ts";
import { scratch } from "./support.ts";

it("drives the mayor alone through chat, its gate, house, reflection, spend, stop and a crashed server (step 2)", async () => {
    const home = scratch();
    const folder = scratch();
    const first = await server(home, {
        responses: [
            {
                calls: [
                    {
                        name: "write",
                        arguments: { path: "a.txt", content: "one" },
                    },
                ],
            },
            {
                calls: [
                    {
                        name: "write",
                        arguments: {
                            path: "NOTES.md",
                            content: "# Notes\nRemembered the folder.\n",
                        },
                    },
                ],
            },
            "Done.",
            {
                calls: [
                    {
                        name: "write",
                        arguments: {
                            path: "NOTES.md",
                            content: "# Notes\nReflected on the work.\n",
                        },
                    },
                ],
            },
            "Remembered.",
            {
                calls: [
                    {
                        name: "bash",
                        arguments: { command: "echo started; sleep 30" },
                    },
                ],
            },
            {
                calls: [
                    {
                        name: "write",
                        arguments: { path: "b.txt", content: "after restart" },
                    },
                ],
            },
        ],
    });
    const chat = hamlet(["chat"], home);
    chat.child.stdin.write(`/folder ${folder}\n/mode manual\nMake a file.\n`);
    await vi.waitFor(
        () =>
            expect(chat.err()).toContain(
                "NEEDS YOU: [approval] mayor wants to edit `a.txt`",
            ),
        { timeout: 5000 },
    );
    chat.child.stdin.write("/allow\n");
    await vi.waitFor(() => expect(chat.out()).toContain("Done.\n"), {
        timeout: 5000,
    });
    expect(readFileSync(join(folder, "a.txt"), "utf8")).toBe("one");
    expect(readFileSync(join(home, "houses/home/0/NOTES.md"), "utf8")).toBe(
        "# Notes\nRemembered the folder.\n",
    );
    expect(chat.err().match(/NEEDS YOU:/g)).toHaveLength(1);
    chat.child.stdin.write("/reflect\n/budget\n");
    await vi.waitFor(() => expect(chat.out()).toContain("Remembered.\n"), {
        timeout: 5000,
    });
    expect(readFileSync(join(home, "houses/home/0/NOTES.md"), "utf8")).toBe(
        "# Notes\nReflected on the work.\n",
    );
    await vi.waitFor(() => expect(chat.err()).toContain("Hamlet today:"), {
        timeout: 5000,
    });
    expect(chat.err().match(/NEEDS YOU:/g)).toHaveLength(1);
    chat.child.stdin.write("/mode auto\nRun a long command.\n");
    await vi.waitFor(
        () =>
            expect(chat.err()).toContain(
                '[bash] {"command":"echo started; sleep 30"}',
            ),
        { timeout: 5000 },
    );
    chat.child.stdin.write("/stop\n");
    await vi.waitFor(
        () => expect(chat.err()).toContain("Stopped: stopped by chief"),
        { timeout: 2500 },
    );
    chat.child.stdin.write("/mode manual\nMake another file.\n");
    await vi.waitFor(
        () =>
            expect(chat.err()).toContain(
                "NEEDS YOU: [approval] mayor wants to edit `b.txt`",
            ),
        { timeout: 5000 },
    );
    const before = await connectChief(await reachHome(home, {}));
    const wait = before.state.waits[0];
    await before.connection.close();
    expect(wait).toMatchObject({ kind: "approval", danger: "edit", asked: 1 });
    first.child.kill("SIGKILL");
    expect(await first.ended).toBe("SIGKILL");
    expect(await chat.ended).toBe(1);
    const again = await server(home, { responses: ["After restart."] });
    const after = await connectChief(await reachHome(home, {}));
    expect(after.state.waits[0]).toMatchObject({
        id: wait?.id,
        since: wait?.since,
        asked: 1,
        answers: ["allow", "always", "deny"],
    });
    await after.connection.close();
    const next = hamlet(["chat"], home);
    await vi.waitFor(
        () =>
            expect(next.err()).toContain(
                "NEEDS YOU: [approval] mayor wants to edit `b.txt`",
            ),
        { timeout: 5000 },
    );
    next.child.stdin.write("/allow\n");
    await vi.waitFor(() => expect(next.out()).toContain("After restart.\n"), {
        timeout: 5000,
    });
    next.child.stdin.end();
    expect(await next.ended).toBe(0);
    expect(readFileSync(join(folder, "b.txt"), "utf8")).toBe("after restart");
    expect(again.child.exitCode).toBeNull();
}, 30_000);
