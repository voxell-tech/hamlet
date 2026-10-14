import { timingSafeEqual } from "node:crypto";
import {
    request as httpRequest,
    type IncomingHttpHeaders,
    type IncomingMessage,
    type RequestListener,
    type ServerResponse,
} from "node:http";
import { request as httpsRequest } from "node:https";
import { Readable } from "node:stream";
import { createGunzip } from "node:zlib";
import { FLUSH, pkt, readSection } from "./pktline.ts";

export type GitProxyOptions = {
    /** The repository's URL at the git host. */
    upstream: string;
    /** The chief's token for the git host. */
    token: string;
    /** The hamlet's own credential, the only one the proxy accepts. */
    credential: string;
};

/** One ref update a push asks for. */
type Command = { old: string; new: string; ref: string };

const ROUTES = new Set([
    "GET /info/refs",
    "POST /git-upload-pack",
    "POST /git-receive-pack",
]);

/** Git's smart HTTP for one repository: checks the hamlet's credential, refuses deleting refs and pushing tags, and adds the chief's token on the way to the git host (S10, ADR 0007). */
export function gitProxy(options: GitProxyOptions): RequestListener {
    return (request, response) => {
        handle(options, request, response).catch((error: unknown) => {
            if (!response.headersSent) response.writeHead(502);
            response.end(String(error));
        });
    };
}

async function handle(
    options: GitProxyOptions,
    request: IncomingMessage,
    response: ServerResponse,
): Promise<void> {
    if (!authorized(request.headers, options.credential)) {
        response.writeHead(401, { "WWW-Authenticate": 'Basic realm="Hamlet"' });
        response.end();
        return;
    }
    const url = new URL(request.url ?? "/", "http://proxy");
    const service = url.searchParams.get("service");
    const known =
        ROUTES.has(`${request.method} ${url.pathname}`) &&
        (url.pathname !== "/info/refs" ||
            service === "git-upload-pack" ||
            service === "git-receive-pack");
    if (!known) {
        response.writeHead(404);
        response.end();
        return;
    }
    let body: Readable = request;
    const headers = { ...request.headers };
    if (url.pathname === "/git-receive-pack") {
        if (headers["content-encoding"] === "gzip") {
            body = request.pipe(createGunzip());
            delete headers["content-encoding"];
            delete headers["content-length"];
        }
        const chunks = body[Symbol.asyncIterator]();
        const { lines, read } = await readSection(chunks);
        const commands = lines.flatMap((line) => command(line.toString()));
        const capabilities = (lines[0]?.toString().split("\0")[1] ?? "").split(
            " ",
        );
        const refused = commands.some((each) => refusal(each) !== undefined);
        if (refused || lines[0]?.toString().startsWith("push-cert")) {
            // Read to the end, so the client hears the answer, but keep none
            // of the pack.
            for (
                let next = await chunks.next();
                !next.done;
                next = await chunks.next()
            ) {}
            refuse(response, commands, capabilities);
            return;
        }
        body = Readable.from(rest(read, chunks));
    }
    forward(options, request.method ?? "GET", url, headers, body, response);
}

function authorized(headers: IncomingHttpHeaders, credential: string): boolean {
    const [scheme, encoded] = (headers.authorization ?? "").split(" ");
    if (scheme !== "Basic" || encoded === undefined) return false;
    const decoded = Buffer.from(encoded, "base64").toString();
    const given = Buffer.from(decoded.slice(decoded.indexOf(":") + 1));
    const expected = Buffer.from(credential);
    return given.length === expected.length && timingSafeEqual(given, expected);
}

/** The ref update a command line names; none for the lines that are not one, such as `shallow`. */
function command(line: string): Command[] {
    const [update = ""] = line.replace(/\n$/, "").split("\0");
    const match = /^([0-9a-f]{40,64}) ([0-9a-f]{40,64}) (\S+)$/.exec(update);
    return match
        ? [{ old: match[1] ?? "", new: match[2] ?? "", ref: match[3] ?? "" }]
        : [];
}

/** The reason `command` is refused, or `undefined` when it may go on. */
function refusal(command: Command): string | undefined {
    if (command.ref.startsWith("refs/tags/")) return "pushing tags is refused";
    if (/^0+$/.test(command.new)) return "deleting branches is refused";
    return undefined;
}

/** Answers a push with every ref rejected, each with its reason. */
function refuse(
    response: ServerResponse,
    commands: Command[],
    capabilities: string[],
): void {
    if (!capabilities.some((each) => each.startsWith("report-status"))) {
        response.writeHead(403);
        response.end("Deleting branches and pushing tags are refused.");
        return;
    }
    const status = Buffer.concat([
        pkt("unpack ok\n"),
        ...commands.map((each) =>
            pkt(
                `ng ${each.ref} ${refusal(each) ?? "refused with the rest of the push"}\n`,
            ),
        ),
        FLUSH,
    ]);
    const sideband =
        capabilities.includes("side-band-64k") ||
        capabilities.includes("side-band");
    response.writeHead(200, {
        "Content-Type": "application/x-git-receive-pack-result",
        "Cache-Control": "no-cache",
    });
    response.end(
        sideband
            ? Buffer.concat([
                  pkt(Buffer.concat([Buffer.from([1]), status])),
                  FLUSH,
              ])
            : status,
    );
}

/** The bytes already read, then the rest of the stream. */
async function* rest(
    read: Buffer,
    chunks: AsyncIterator<Buffer>,
): AsyncGenerator<Buffer> {
    yield read;
    for (
        let next = await chunks.next();
        !next.done;
        next = await chunks.next()
    ) {
        yield next.value;
    }
}

/** Sends the request on to the git host with the chief's token, and its answer back. */
function forward(
    options: GitProxyOptions,
    method: string,
    url: URL,
    headers: IncomingHttpHeaders,
    body: Readable,
    response: ServerResponse,
): void {
    const target = new URL(`${options.upstream}${url.pathname}${url.search}`);
    const send = target.protocol === "https:" ? httpsRequest : httpRequest;
    const upstream = send(target, {
        method,
        headers: {
            ...pick(headers, [
                "accept",
                "accept-encoding",
                "content-encoding",
                "content-length",
                "content-type",
                "git-protocol",
                "user-agent",
            ]),
            authorization: `Basic ${Buffer.from(`x-access-token:${options.token}`).toString("base64")}`,
        },
    });
    upstream.on("response", (answer) => {
        response.writeHead(
            answer.statusCode ?? 502,
            pick(answer.headers, [
                "cache-control",
                "content-encoding",
                "content-length",
                "content-type",
            ]),
        );
        answer.pipe(response);
    });
    upstream.on("error", (error) => {
        if (!response.headersSent) response.writeHead(502);
        response.end(String(error));
    });
    body.pipe(upstream);
}

function pick(
    headers: IncomingHttpHeaders,
    names: string[],
): Record<string, string | string[]> {
    const picked: Record<string, string | string[]> = {};
    for (const name of names) {
        const value = headers[name];
        if (value !== undefined) picked[name] = value;
    }
    return picked;
}
