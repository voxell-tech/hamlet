import { execFile, spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import { createServer, type RequestListener, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { join } from "node:path";
import { scratch } from "./support.ts";

/** The end of a git command. */
export type Ran = { code: number; stdout: string; stderr: string };

/** Runs git with no user or system configuration and no prompts. */
export function git(args: string[], cwd?: string): Promise<Ran> {
    const home = scratch();
    return new Promise((resolve) => {
        execFile(
            "git",
            args,
            {
                cwd,
                env: {
                    PATH: process.env.PATH ?? "",
                    HOME: home,
                    GIT_CONFIG_NOSYSTEM: "1",
                    GIT_CONFIG_GLOBAL: join(home, "gitconfig"),
                    GIT_TERMINAL_PROMPT: "0",
                    GIT_AUTHOR_NAME: "Chief",
                    GIT_AUTHOR_EMAIL: "chief@example.com",
                    GIT_COMMITTER_NAME: "Chief",
                    GIT_COMMITTER_EMAIL: "chief@example.com",
                },
            },
            (error, stdout, stderr) => {
                const code =
                    error === null
                        ? 0
                        : typeof error.code === "number"
                          ? error.code
                          : 1;
                resolve({ code, stdout, stderr });
            },
        );
    });
}

/** Serves `listener` on a free local port. */
export async function serve(
    listener: RequestListener,
): Promise<{ url: string; server: Server }> {
    const server = createServer(listener);
    await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
    const { port } = server.address() as AddressInfo;
    return { url: `http://127.0.0.1:${port}`, server };
}

/** A git host on this machine, as GitHub would be: `project.git` with `main` and `feature`, served by `git http-backend` to whoever has `token`. */
export async function gitHost(token: string): Promise<{
    url: string;
    repo: string;
    /** The requests the host was sent, as `METHOD path`. */
    requests: string[];
    close(): void;
}> {
    const root = scratch();
    const repo = join(root, "project.git");
    const work = scratch();
    await git(["init", "-q", "--bare", "--initial-branch=main", repo]);
    await git(["init", "-q", "--initial-branch=main", work]);
    writeFileSync(join(work, "README.md"), "Hello\n");
    await git(["add", "README.md"], work);
    await git(["commit", "-q", "-m", "Start"], work);
    await git(["push", "-q", repo, "main", "main:feature"], work);

    const requests: string[] = [];
    const expected = `Basic ${Buffer.from(`x-access-token:${token}`).toString("base64")}`;
    const { url, server } = await serve((request, response) => {
        const at = new URL(request.url ?? "/", "http://host");
        requests.push(`${request.method} ${at.pathname}`);
        if (request.headers.authorization !== expected) {
            response.writeHead(401, {
                "WWW-Authenticate": 'Basic realm="git"',
            });
            response.end();
            return;
        }
        const backend = spawn("git", ["http-backend"], {
            env: {
                PATH: process.env.PATH ?? "",
                GIT_PROJECT_ROOT: root,
                GIT_HTTP_EXPORT_ALL: "1",
                PATH_INFO: at.pathname,
                QUERY_STRING: at.search.slice(1),
                REQUEST_METHOD: request.method ?? "GET",
                CONTENT_TYPE: request.headers["content-type"] ?? "",
                HTTP_CONTENT_ENCODING:
                    request.headers["content-encoding"] ?? "",
                REMOTE_USER: "chief",
                ...(request.headers["content-length"] === undefined
                    ? {}
                    : { CONTENT_LENGTH: request.headers["content-length"] }),
            },
        });
        request.pipe(backend.stdin);
        respondCgi(backend.stdout, response);
    });
    return {
        url,
        repo,
        requests,
        close: () => server.close(),
    };
}

/** Sends a CGI program's output as the HTTP response: its headers, then its body. */
function respondCgi(
    output: NodeJS.ReadableStream,
    response: Parameters<RequestListener>[1],
): void {
    let head = Buffer.alloc(0);
    let sent = false;
    output.on("data", (chunk: Buffer) => {
        if (sent) {
            response.write(chunk);
            return;
        }
        head = Buffer.concat([head, chunk]);
        const end = head.indexOf("\r\n\r\n");
        if (end === -1) return;
        let status = 200;
        const headers: Record<string, string> = {};
        for (const line of head.subarray(0, end).toString().split("\r\n")) {
            const colon = line.indexOf(":");
            const name = line.slice(0, colon).trim();
            const value = line.slice(colon + 1).trim();
            if (name.toLowerCase() === "status")
                status = Number.parseInt(value, 10);
            else headers[name] = value;
        }
        response.writeHead(status, headers);
        response.write(head.subarray(end + 4));
        sent = true;
    });
    output.on("end", () => response.end());
}

/** The commit `ref` names in the host's repository, or "" when there is none. */
export async function hostRef(repo: string, ref: string): Promise<string> {
    return (
        await git(["--git-dir", repo, "rev-parse", "--verify", "-q", ref])
    ).stdout.trim();
}
