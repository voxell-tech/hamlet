import { rename, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { chat } from "./chat.ts";
import { reachHome } from "./client.ts";
import { FAUX_MODEL, readScript, scriptedModels } from "./faux.ts";
import { HOME, MAYOR } from "./hamlet.ts";
import { openHome } from "./home.ts";
import { probeSandbox } from "./sandbox/probe.ts";
import { serveHome } from "./server.ts";
import {
    DOCUMENTS,
    type DocumentName,
    type TerminalCommand,
    terminal,
} from "./terminal.ts";

/** The port a server listens on unless it is given another. */
const PORT = 7313;

const USAGE = `usage: hamlet server [--port <n>] [--faux <script.json>]
       hamlet chat [--to <agent>]
       hamlet edit <charter|character|notes|limits|settings>
       hamlet login <provider> [--key]
       hamlet models [try [model] | refresh]

  server   serve the home to its chief's clients; Ctrl-C stops
    --port   the port on 127.0.0.1 (${PORT}; 0 for any free one)
    --faux   answer with the responses of a script instead of a model
  chat     chat with the hamlet's mayor in the terminal; Ctrl-D quits
    --to     chat with that agent instead
  edit     open the chief's document in EDITOR, then save it through the server
  login    paste a key with --key, or follow the provider's sign-in
  models   list models, refresh the list, or try a one-word hello

env: HAMLET_HOME (~/.hamlet), HAMLET_USER ($USER);
     HAMLET_SERVER and HAMLET_TOKEN, for a chat with a server away from its home;
     HAMLET_BASE_URL, HAMLET_MODEL, HAMLET_API_KEY, HAMLET_REFLECT_EVERY, HAMLET_NET override the settings.`;

/** The value each flag of a command needs. */
const FLAGS = {
    server: { "--port": "a port number", "--faux": "the path of a script" },
    chat: { "--to": "an agent's name" },
} as const;

type Command =
    | { name: "server"; port: number; faux: string | undefined }
    | { name: "chat"; to: string }
    | TerminalCommand;

const context = BACKGROUND_CONTEXT;

try {
    const command = parse(process.argv.slice(2));
    if (command === undefined) process.stdout.write(`${USAGE}\n`);
    else if (command.name === "server") await serve(command);
    else if (command.name === "chat") await talk(command.to);
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
        const [doc, extra] = rest;
        if (extra !== undefined)
            throw new Error(`unknown argument \`${extra}\`\n\n${USAGE}`);
        if (!DOCUMENTS.some((name) => name === doc))
            throw new Error(
                `edit needs one of charter, character, notes, limits or settings\n\n${USAGE}`,
            );
        return { name, doc: doc as DocumentName };
    }
    if (name !== "server" && name !== "chat") {
        throw new Error(
            name === undefined
                ? `say which: \`hamlet server\` or \`hamlet chat\`\n\n${USAGE}`
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
    const port = given.get("--port") ?? String(PORT);
    if (!/^[0-9]{1,5}$/.test(port) || Number(port) > 65_535) {
        throw new Error(
            `--port needs a port number, not \`${port}\`\n\n${USAGE}`,
        );
    }
    return { name, port: Number(port), faux: given.get("--faux") };
}

/** The home this process works on (C8). */
function homeDir(): string {
    return process.env.HAMLET_HOME || join(homedir(), ".hamlet");
}

/** A home served until a signal closes it in order; a second signal exits at once (S1, S3). */
async function serve(command: { port: number; faux: string | undefined }) {
    probeSandbox();
    const faux =
        command.faux === undefined
            ? undefined
            : scriptedModels(await readScript(command.faux));
    const home = await openHome(
        homeDir(),
        faux === undefined
            ? {}
            : {
                  models: faux,
                  env: { ...process.env, HAMLET_MODEL: FAUX_MODEL },
              },
        context,
    );
    const server = await serveHome(home, command.port, context).catch(
        async (error: unknown) => {
            await home.close(context);
            throw error;
        },
    );
    // The address lands whole; the token stays in credentials.json.
    const written = `${home.layout.server}.${process.pid}.tmp`;
    await writeFile(
        written,
        `${JSON.stringify({ url: server.url, pid: process.pid })}\n`,
    );
    await rename(written, home.layout.server);
    process.stderr.write(
        `hamlet: serving ${home.dir} at ${server.url}; Ctrl-C stops\n`,
    );
    void home.harness.closed.then((end) => {
        if (end.reason !== "failed") return;
        process.stderr.write(
            `Error: the home's storage failed: ${end.error}\n`,
        );
        process.exit(1);
    });
    let stopping = false;
    const stop = () => {
        if (stopping) process.exit(130);
        stopping = true;
        process.stderr.write("hamlet: stopping; Ctrl-C again stops at once\n");
        void (async () => {
            await rm(home.layout.server, { force: true });
            await server.close();
            await home.close(context);
            process.exit(0);
        })().catch((error: unknown) => {
            process.stderr.write(`Error: ${String(error)}\n`);
            process.exit(1);
        });
    };
    process.on("SIGINT", stop);
    process.on("SIGTERM", stop);
}

/** A chat with `to` through the local or environment-named server (S4, S13). */
async function talk(to: string) {
    const reached = await reachHome(homeDir(), process.env);
    await chat({
        ...reached,
        to: { hamlet: HOME, agent: to },
        input: process.stdin,
        out: process.stdout,
        err: process.stderr,
    });
}
