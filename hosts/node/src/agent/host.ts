/**
 * The agent actor a Node host serves on the realm folder `home`, its models
 * the faux ones of `faux.ts`, or today's providers and the realm's endpoints
 * where it has no faux script and lists no sign-in on loopback. It writes the
 * process's pid in the folder.
 */
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Provider } from "@earendil-works/pi-ai/models";
import { type AgentConfig, type AgentDefinition, agentActor } from "./actor.ts";
import { claudeExecutable, withClaude } from "./claude.ts";
import { fauxModels } from "./faux.ts";
import type { AgentFilesystem } from "./filesystem.ts";
import { installationOf, signIns } from "./login.ts";
import { openAgentFilesystem } from "./native-filesystem.ts";
import { hostProviders, servingEndpoints } from "./providers.ts";

const PID_FILE = "demo-node.pid";

export interface HostedAgent {
    /** The faux script the model answers from first. */
    readonly faux?: string;
    /** Lists two providers on loopback to sign in to. */
    readonly signIn: boolean;
    /** Adds the Claude program's models, where it is installed. */
    readonly claude: boolean;
    /** The providers in place of today's, where it has no faux script and lists no sign-in. */
    readonly providers?: readonly Provider[];
    readonly files?: (files: AgentFilesystem) => AgentFilesystem;
}

export function hostedAgent(home: string, host: HostedAgent): AgentDefinition {
    writeFileSync(join(home, PID_FILE), `${process.pid}\n`);
    return agentActor(hostedConfig(home, host));
}

/** The agent's config on the realm folder `home`. */
export function hostedConfig(home: string, host: HostedAgent): AgentConfig {
    const { models, model } = fauxModels(host.faux, host.signIn);
    const real = host.faux === undefined && !host.signIn;
    if (real) {
        // Today's providers stand in the faux ones' place; the faux model's
        // name stays the model of an agent the realm gave none.
        models.clearProviders();
        for (const provider of host.providers ?? hostProviders())
            models.setProvider(provider);
    }
    const executable = host.claude ? claudeExecutable() : undefined;
    if (executable !== undefined)
        withClaude(models, { executable, folder: join(home, "claude") });
    const given = host.files ?? ((files) => files);
    return {
        models,
        model,
        listed: () => models.getModels(),
        files: (n) => given(openAgentFilesystem(home, n)),
        library: join(home, "library"),
        signIns: signIns(models, installationOf(home)),
        faux: host.faux !== undefined,
        ...(real ? { serve: servingEndpoints(models) } : {}),
    };
}
