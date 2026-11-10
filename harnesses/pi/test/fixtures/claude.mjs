// A fake Claude program for tests: it answers the Agent SDK's stream-json as the real one does, running the steps of
// the script `FAKE_CLAUDE_SCRIPT` names, one run per process, and never reaches a network. Like the real one, it
// writes what each run says to its session, the one it resumes or a new one.
import { randomUUID } from "node:crypto";
import {
    appendFileSync,
    existsSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { createInterface } from "node:readline";
import { getSessionPath, normalizeProjectPath } from "cc-session-io";

const scriptFile = process.env.FAKE_CLAUDE_SCRIPT ?? "";
const script = JSON.parse(readFileSync(scriptFile, "utf8"));
const args = process.argv.slice(2);
const config = process.env.CLAUDE_CONFIG_DIR ?? "";

/** Notes what this run was given, for the test to read. */
function note(what) {
    appendFileSync(script.log, `${JSON.stringify(what)}\n`);
}

function out(message) {
    process.stdout.write(`${JSON.stringify(message)}\n`);
}

/** The file of the session `id` in the Claude folder. */
function sessionFile(id) {
    const projects = join(config, "projects");
    for (const project of existsSync(projects) ? readdirSync(projects) : []) {
        const file = join(projects, project, `${id}.jsonl`);
        if (existsSync(file)) return file;
    }
    return undefined;
}

const resumed = args
    .find((arg) => arg.startsWith("--resume="))
    ?.slice("--resume=".length);
const session = resumed ?? randomUUID();
const records =
    (resumed === undefined ? undefined : sessionFile(resumed)) ??
    getSessionPath(session, normalizeProjectPath(process.cwd()), config);

/** Writes `record` to the session. */
function record(what) {
    mkdirSync(dirname(records), { recursive: true });
    appendFileSync(records, `${JSON.stringify(what)}\n`);
}
const counter = `${scriptFile}.runs`;
const run = existsSync(counter) ? Number(readFileSync(counter, "utf8")) : 0;
writeFileSync(counter, String(run + 1));
const waiting = new Map();
let next = 0;

/** Asks the SDK's MCP server named hamlet, and gives its answer. */
function mcp(method, params) {
    next += 1;
    const id = `fake-${next}`;
    out({
        type: "control_request",
        request_id: id,
        request: {
            subtype: "mcp_message",
            server_name: "hamlet",
            message: { jsonrpc: "2.0", id: next, method, params },
        },
    });
    return new Promise((resolve) => waiting.set(id, resolve));
}

async function steps(prompt) {
    const file = resumed === undefined ? undefined : sessionFile(resumed);
    note({
        run,
        prompt,
        session,
        resume: resumed ?? null,
        resumed: file === undefined ? null : readFileSync(file, "utf8"),
        config,
        apiKey: process.env.ANTHROPIC_API_KEY ?? null,
    });
    out({
        type: "system",
        subtype: "init",
        session_id: session,
        uuid: randomUUID(),
        tools: [],
        mcp_servers: [{ name: "hamlet", status: "connected" }],
        model: "claude-fake",
        permissionMode: "default",
        apiKeySource: "none",
        cwd: process.cwd(),
        slash_commands: [],
        output_style: "default",
    });
    record({ type: "user", message: { role: "user", content: prompt } });
    for (const step of script.runs[run] ?? []) {
        if (step.call !== undefined) {
            const answer = await mcp("tools/call", {
                name: step.call.name,
                arguments: step.call.arguments,
                _meta: { "claudecode/toolUseId": step.call.id },
            });
            note({ run, result: answer.result });
            record({
                type: "user",
                message: {
                    role: "user",
                    content: [
                        {
                            type: "tool_result",
                            tool_use_id: step.call.id,
                            content: answer.result.content,
                        },
                    ],
                },
            });
            continue;
        }
        record(step.say);
        out({ uuid: randomUUID(), ...step.say, session_id: session });
    }
}

createInterface({ input: process.stdin }).on("line", (line) => {
    const message = JSON.parse(line);
    if (message.type === "control_request") {
        out({
            type: "control_response",
            response: {
                subtype: "success",
                request_id: message.request_id,
                response:
                    message.request.subtype === "initialize"
                        ? {
                              commands: [],
                              agents: [],
                              output_style: "default",
                              available_output_styles: ["default"],
                              models: [],
                              account: {},
                          }
                        : {},
            },
        });
    } else if (message.type === "control_response") {
        const resolve = waiting.get(message.response.request_id);
        waiting.delete(message.response.request_id);
        resolve?.(message.response.response.mcp_response);
    } else if (message.type === "user") {
        void steps(message.message.content);
    }
});
