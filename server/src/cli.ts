import { lstat, mkdir, realpath } from "node:fs/promises";
import { constants, homedir, tmpdir, userInfo } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { chat } from "./chat.ts";
import { connectChief, reachHome } from "./client.ts";
import { HOME, MAYOR } from "./hamlet.ts";
import { serve } from "./serving.ts";
import {
    DOCUMENTS,
    type DocumentName,
    type TerminalCommand,
    terminal,
} from "./terminal.ts";
import { findApp, launch } from "./window.ts";

/** The port a server listens on unless it is given another. */
const PORT = 7313;

const USAGE = `usage: hamlet server [--port <n>] [--faux <script.json>]
       hamlet chat [--to <agent|name@hamlet|mayor@hamlet>]
       hamlet dev [--port <n>] [--faux <script.json>] [--home <dir>]
       hamlet window
       hamlet edit <charter|character|notes|limits|settings> [--hamlet <id>]
       hamlet login <provider> [--key]
       hamlet models [try [model] | refresh]

  server   serve the home to its chief's clients; Ctrl-C stops
    --port   the port on 127.0.0.1 (${PORT}; 0 for any free one)
    --faux   answer with the responses of a script instead of a model
  chat     chat with the hamlet's mayor in the terminal; Ctrl-D quits
    --to     chat with that agent anywhere in the realm instead
  dev      serve a scratch home on a free port and open its window; Ctrl-C stops
    --home   use this scratch home instead of HAMLET_HOME or the per-user temporary home
  window   open a window connected to a running server
  edit     open the chief's document in EDITOR, then save it through the server
    --hamlet choose a hamlet for edit limits (home by default)
  login    paste a key with --key, or follow the provider's sign-in
  models   list models, refresh the list, or try a one-word hello

env: HAMLET_HOME (~/.hamlet), HAMLET_USER ($USER);
     HAMLET_SERVER and HAMLET_TOKEN, for clients away from the server's home;
     HAMLET_APP finds hamlet-app; HAMLET_VIEW names the window's own folder;
     HAMLET_BASE_URL, HAMLET_MODEL, HAMLET_API_KEY, HAMLET_REFLECT_EVERY, HAMLET_NET override the settings.`;

const SERVER_FLAGS = {
    "--port": "a port number",
    "--faux": "the path of a script",
};

/** The value each flag of a command needs. */
const FLAGS = {
    server: SERVER_FLAGS,
    dev: { ...SERVER_FLAGS, "--home": "the path of a scratch home" },
    chat: { "--to": "an agent's name" },
} as const;

type ServingCommand = {
    port: number;
    faux: string | undefined;
};

type Command =
    | ({ name: "server" } & ServingCommand)
    | ({ name: "dev"; home: string | undefined } & ServingCommand)
    | { name: "chat"; to: string }
    | { name: "window" }
    | TerminalCommand;

try {
    const command = parse(process.argv.slice(2));
    if (command === undefined) process.stdout.write(`${USAGE}\n`);
    else if (command.name === "server") await serve(homeDir(), command);
    else if (command.name === "dev") await develop(command);
    else if (command.name === "chat") await talk(command.to);
    else if (command.name === "window") await openWindow();
    else await terminal(command, await reachHome(homeDir(), process.env));
} catch (error) {
    process.stderr.write(
        `Error: ${error instanceof Error ? error.message : String(error)}\n`,
    );
    process.exitCode = 1;
}

/** The requested command, or `undefined` for help. */
function parse(args: readonly string[]): Command | undefined {
    if (args.some((arg) => arg === "-h" || arg === "--help")) return undefined;
    const [name, ...rest] = args;
    if (name === "window") {
        if (rest.length !== 0)
            throw new Error(`unknown argument \`${rest[0]}\`\n\n${USAGE}`);
        return { name };
    }
    if (name === "models") {
        const [action, model, extra] = rest;
        if (action === undefined) return { name, action: "list" };
        if (action === "try" && extra === undefined)
            return { name, action, ...(model === undefined ? {} : { model }) };
        if (action === "refresh" && model === undefined)
            return { name, action };
        throw new Error(
            `unknown argument \`${extra ?? (action === "try" || action === "refresh" ? model : action)}\`\n\n${USAGE}`,
        );
    }
    if (name === "login") {
        const [provider, flag, extra] = rest;
        if (!provider || provider.startsWith("--"))
            throw new Error(`login needs a provider\n\n${USAGE}`);
        if (extra !== undefined || (flag !== undefined && flag !== "--key"))
            throw new Error(
                `unknown argument \`${extra ?? flag}\`\n\n${USAGE}`,
            );
        return { name, provider, key: flag === "--key" };
    }
    if (name === "edit") {
        const [doc, flag, hamlet, extra] = rest;
        if (!DOCUMENTS.some((name) => name === doc))
            throw new Error(
                `edit needs one of charter, character, notes, limits or settings\n\n${USAGE}`,
            );
        if (flag !== undefined && (flag !== "--hamlet" || doc !== "limits"))
            throw new Error(`unknown argument \`${flag}\`\n\n${USAGE}`);
        if (extra !== undefined)
            throw new Error(`unknown argument \`${extra}\`\n\n${USAGE}`);
        if (flag === "--hamlet" && (!hamlet || hamlet.startsWith("--")))
            throw new Error("--hamlet needs a hamlet id");
        return {
            name,
            doc: doc as DocumentName,
            ...(hamlet === undefined ? {} : { hamlet }),
        };
    }
    if (name !== "server" && name !== "dev" && name !== "chat") {
        throw new Error(
            name === undefined
                ? `choose a command from the usage below\n\n${USAGE}`
                : `unknown argument \`${name}\`\n\n${USAGE}`,
        );
    }
    const takes: { readonly [flag: string]: string } = FLAGS[name];
    const given = new Map<string, string>();
    for (let i = 0; i < rest.length; i += 2) {
        const flag = rest[i] ?? "";
        const needs = takes[flag];
        if (needs === undefined) {
            throw new Error(`unknown argument \`${flag}\`\n\n${USAGE}`);
        }
        const value = rest[i + 1];
        if (value === undefined) {
            throw new Error(`${flag} needs ${needs}\n\n${USAGE}`);
        }
        given.set(flag, value);
    }
    if (name === "chat") return { name, to: given.get("--to") ?? MAYOR };
    const port = given.get("--port") ?? String(name === "dev" ? 0 : PORT);
    if (!/^[0-9]{1,5}$/.test(port) || Number(port) > 65_535) {
        throw new Error(
            `--port needs a port number, not \`${port}\`\n\n${USAGE}`,
        );
    }
    const serving = { port: Number(port), faux: given.get("--faux") };
    return name === "dev"
        ? { name, ...serving, home: given.get("--home") }
        : { name, ...serving };
}

/** The home this process works on (C8). */
function homeDir(): string {
    return process.env.HAMLET_HOME || join(homedir(), ".hamlet");
}

/** A scratch server whose agents keep working after its window closes (S1, S13). */
async function develop(
    command: Extract<Command, { name: "dev" }>,
): Promise<void> {
    const home =
        command.home ||
        process.env.HAMLET_HOME ||
        join(tmpdir(), `hamlet-dev-${userInfo().uid}`);
    if (
        home === "~/.hamlet" ||
        (await canonical(home)) ===
            (await canonical(join(homedir(), ".hamlet")))
    ) {
        throw new Error(
            "hamlet dev refuses ~/.hamlet; choose a scratch home with --home or HAMLET_HOME.",
        );
    }
    await mkdir(resolve(home), { recursive: true, mode: 0o700 }).catch(
        (error: NodeJS.ErrnoException) => {
            if (error.code !== "EEXIST") throw error;
        },
    );
    const stat = await lstat(resolve(home));
    const unsafe = stat.isSymbolicLink()
        ? "a symbolic link"
        : !stat.isDirectory()
          ? "not a directory"
          : stat.uid !== userInfo().uid
            ? "not owned by the current user"
            : (stat.mode & 0o022) !== 0
              ? "writable by group or others"
              : undefined;
    if (unsafe !== undefined)
        throw new Error(`unsafe scratch home ${home}: ${unsafe}.`);
    const app = findApp(process.env);
    let child: ReturnType<typeof launch> | undefined;
    let stopping = false;
    const serving = await serve(home, {
        ...command,
        beforeClose: () => {
            stopping = true;
            if (
                child === undefined ||
                child.exitCode !== null ||
                child.signalCode !== null
            )
                return;
            child.kill("SIGTERM");
        },
    });
    child = launch(app, { ...serving, view: join(serving.home, "window") });
    child.once("exit", () => {
        if (!stopping)
            process.stderr.write(
                `hamlet: window closed; still serving ${serving.home} at ${serving.url}; Ctrl-C stops\n`,
            );
    });
    try {
        await new Promise<void>((resolve, reject) => {
            child.once("spawn", resolve);
            child.once("error", reject);
        });
    } catch (error) {
        await serving.close();
        throw error;
    }
}

/** An absolute home path with existing symbolic links resolved. */
async function canonical(dir: string): Promise<string> {
    const absolute = resolve(dir);
    try {
        return await realpath(absolute);
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
        return join(await canonical(dirname(absolute)), basename(absolute));
    }
}

/** A window connected to the local or environment-named server (S4, S13). */
async function openWindow(): Promise<void> {
    const address = await reachHome(homeDir(), process.env);
    const child = launch(findApp(process.env), address);
    process.exitCode = await new Promise<number>((resolve, reject) => {
        child.once("error", reject);
        child.once("exit", (code, signal) =>
            resolve(
                code ?? (signal === null ? 1 : 128 + constants.signals[signal]),
            ),
        );
    });
}

/** A chat with `to` through the local or environment-named server (S4, S13). */
async function talk(to: string) {
    const reached = await reachHome(homeDir(), process.env);
    const [agent = "", hamlet, extra] = to.split("@");
    if (!agent || extra !== undefined || hamlet === "")
        throw new Error("--to names an agent, name@hamlet or mayor@hamlet");
    let target = { hamlet: hamlet ?? HOME, agent };
    if (hamlet === undefined && agent !== MAYOR) {
        const joined = await connectChief(reached);
        try {
            const found = joined.state.agents.find(
                (a) => a.name === agent && !a.left,
            );
            if (!found) throw new Error(`no agent named \`${agent}\``);
            target = {
                hamlet: found.ref.hamlet,
                agent: found.mayor ? MAYOR : found.name,
            };
        } finally {
            await joined.connection.close();
        }
    }
    await chat({
        ...reached,
        to: target,
        input: process.stdin,
        out: process.stdout,
        err: process.stderr,
    });
}
