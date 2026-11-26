/**
 * The agent actor a Node host serves on the realm folder `home`, its model
 * the faux one of `faux.ts`. It writes the process's pid in the folder.
 */
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { type AgentDefinition, agentActor } from "./actor.ts";
import { claudeExecutable, withClaude } from "./claude.ts";
import { fauxModels } from "./faux.ts";
import type { AgentFilesystem } from "./filesystem.ts";
import { signIns } from "./login.ts";
import { openAgentFilesystem } from "./native-filesystem.ts";

/** A kill test reads it to tell this process from the next one. */
const PID_FILE = "demo-node.pid";

export interface HostedAgent {
    /** The faux script the model answers from first. */
    readonly faux?: string;
    /** Lists two providers on loopback to sign in to. */
    readonly signIn: boolean;
    /** Adds the Claude program's models, where it is installed. */
    readonly claude: boolean;
    /** The agent's filesystem as the host gives it. */
    readonly files?: (files: AgentFilesystem) => AgentFilesystem;
}

export function hostedAgent(home: string, host: HostedAgent): AgentDefinition {
    writeFileSync(join(home, PID_FILE), `${process.pid}\n`);
    const { models, model } = fauxModels(host.faux, host.signIn);
    const executable = host.claude ? claudeExecutable() : undefined;
    if (executable !== undefined)
        withClaude(models, { executable, folder: join(home, "claude") });
    const given = host.files ?? ((files) => files);
    return agentActor({
        models,
        model,
        listed: () => models.getModels(),
        files: (n) => given(openAgentFilesystem(home, n)),
        library: join(home, "library"),
        signIns: signIns(models),
        faux: host.faux !== undefined,
    });
}
