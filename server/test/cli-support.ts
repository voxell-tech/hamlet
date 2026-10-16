import {
    type ChildProcessWithoutNullStreams,
    execFile,
    spawn,
} from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { expect, vi } from "vitest";
import type { Script } from "../src/models.ts";
import { closeAfter, scratch } from "./support.ts";

const executable = fileURLToPath(new URL("../bin/hamlet", import.meta.url));

/** A run of the `hamlet` command, with what it printed so far. */
export type Run = {
    readonly child: ChildProcessWithoutNullStreams;
    readonly out: () => string;
    readonly err: () => string;
    /** The exit code, or the signal's name. */
    readonly ended: Promise<number | string>;
};

/** A command process on a scratch home, killed after the test if still running. */
export function hamlet(
    args: string[],
    home: string,
    input?: string,
    env: NodeJS.ProcessEnv = {},
): Run {
    const child = spawn(executable, args, {
        cwd: new URL("..", import.meta.url),
        env: {
            ...process.env,
            HAMLET_HOME: home,
            HAMLET_USER: "chief",
            ...env,
        },
    });
    let out = "";
    let err = "";
    child.stdout.on("data", (data) => {
        out += data;
    });
    child.stderr.on("data", (data) => {
        err += data;
    });
    const ended = new Promise<number | string>((resolve) =>
        child.on("close", (code, signal) => resolve(code ?? signal ?? "")),
    );
    if (input !== undefined) child.stdin.end(input);
    closeAfter(async () => {
        if (child.exitCode === null && child.signalCode === null) {
            child.kill("SIGKILL");
            await ended;
        }
    });
    return { child, out: () => out, err: () => err, ended };
}

/** A faux script in a file of its own. */
function script(content: Script): string {
    const file = join(scratch(), "script.json");
    writeFileSync(file, JSON.stringify(content));
    return file;
}

/** A faux server process, ready once it has written its address. */
export async function server(
    home: string,
    content: Script,
): Promise<Run & { url: string; pid: number }> {
    const run = hamlet(
        ["server", "--port", "0", "--faux", script(content)],
        home,
    );
    const file = join(home, "server.json");
    await vi.waitFor(
        () => {
            expect(existsSync(file)).toBe(true);
            expect(JSON.parse(readFileSync(file, "utf8"))).toMatchObject({
                pid: run.child.pid,
            });
        },
        { timeout: 10_000 },
    );
    const found = JSON.parse(readFileSync(file, "utf8")) as {
        url: string;
        pid: number;
    };
    return { ...run, ...found };
}

/** A real terminal sending each line only after the command's prompt. */
export async function tty(
    args: string[],
    home: string,
    steps: { after: string; write: string }[],
    env: NodeJS.ProcessEnv = {},
): Promise<{ code: number; output: string }> {
    const python = `import json,os,pty,select,subprocess,sys,time
master,slave=pty.openpty()
p=subprocess.Popen(sys.argv[1:],stdin=slave,stdout=slave,stderr=slave)
os.close(slave)
output=b''
steps=json.loads(os.environ['TEST_STEPS'])
try:
 deadline=time.monotonic()+10
 while time.monotonic()<deadline:
  if select.select([master],[],[],0.1)[0]:
   try: data=os.read(master,4096)
   except OSError: break
   if not data: break
   output+=data
   if steps and steps[0]['after'].encode() in output:
    os.write(master,steps.pop(0)['write'].encode())
  elif p.poll() is not None: break
 if p.poll() is None: p.kill()
 p.wait(timeout=1)
 print(json.dumps({'code':p.returncode,'output':output.decode()}))
finally:
 if p.poll() is None: p.kill();p.wait()
 os.close(master)
`;
    const result = await promisify(execFile)(
        "python3",
        ["-c", python, executable, ...args],
        {
            cwd: new URL("..", import.meta.url),
            env: {
                ...process.env,
                HAMLET_HOME: home,
                HAMLET_USER: "chief",
                ...env,
                TEST_STEPS: JSON.stringify(steps),
            },
        },
    );
    return JSON.parse(result.stdout) as { code: number; output: string };
}
