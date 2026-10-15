import type { Context } from "@earendil-works/chord";
import type { Submission } from "@earendil-works/pi-durable";
import { ChatDoc, submitLine } from "./chats.ts";
import { readCommand } from "./commands.ts";
import { answerWords } from "./gate.ts";
import { giftSkill } from "./gifts.ts";
import { GrantsDoc, grantsOf, revoke } from "./grants.ts";
import { agentKey, HOME } from "./hamlet.ts";
import { historyOf } from "./history.ts";
import type { Home } from "./home.ts";
import { resetBudget } from "./ledger.ts";
import { LimitsDoc, limitsOf } from "./limits.ts";
import { safeWords } from "./plainly.ts";
import type { Reply, Say } from "./protocol.ts";
import { addressed } from "./record.ts";
import { reportOf } from "./report.ts";
import { newChat, reflect, resumeChat, retry, stopAgent } from "./talk.ts";
import { OversightDoc } from "./waits.ts";
import { queue, strip } from "./words.ts";
import { folder, setMode } from "./work.ts";

/** A chief's typed line, dispatched as a command or admitted as scrubbed words (H24, C32). */
export async function say(
    home: Home,
    message: Say,
    context: Context,
    submitted?: (submission: Submission) => void,
): Promise<Reply | undefined> {
    if (message.to.hamlet !== HOME)
        throw new Error(`no hamlet named \`${message.to.hamlet}\``);
    const text = safeWords(home, message.text);
    const command = readCommand(text);
    if (command !== undefined && "error" in command)
        return reply(message.id, command.error, command.kind);
    if (command?.command === "undo")
        return reply(
            message.id,
            "Nothing to undo. Only a removal or a merge can be undone (the archive keeps the house); a permission is taken back with /revoke. A command that ran, a file that was changed and what was spent cannot be undone.",
        );
    if (command?.command === "budget") {
        const report = await reportOf(home, message.to.hamlet, context);
        return reply(message.id, [report.summary, ...report.lines].join("\n"));
    }
    if (command?.command === "reset-budget")
        return reply(
            message.id,
            await resetBudget(home, message.to.hamlet, home.chief, context),
        );
    if (command?.command === "grants") {
        const limits = limitsOf(
            await home.harness.snapshot(LimitsDoc, message.to.hamlet, context),
        );
        const grants = grantsOf(
            await home.harness.snapshot(GrantsDoc, context),
        ).filter((grant) => grant.agent.hamlet === message.to.hamlet);
        const lines = Object.entries(limits.permissions).flatMap(
            ([permission, names]) =>
                names.map(
                    (name) => `${name} may use: ${permission} (limits.toml)`,
                ),
        );
        lines.push(
            ...grants.map(
                (grant) =>
                    `${grant.name} may use: ${grant.permission} (given by ${grant.by}; /revoke ${grant.name} ${grant.permission})`,
            ),
        );
        return reply(
            message.id,
            lines.length === 0
                ? "No agent holds a permission."
                : lines.join("\n"),
        );
    }
    if (command?.command === "revoke") {
        const hamlet = message.to.hamlet;
        const agent =
            (await addressed(
                home.harness,
                { hamlet, agent: command.agent },
                context,
            )) ??
            grantsOf(await home.harness.snapshot(GrantsDoc, context)).find(
                (grant) =>
                    grant.agent.hamlet === hamlet &&
                    grant.name === command.agent,
            )?.agent;
        if (agent !== undefined)
            await home.harness.commit(
                (tx) =>
                    revoke(
                        tx,
                        agent,
                        command.permission,
                        home.chief,
                        home.now(),
                    ),
                context,
            );
        const limits = limitsOf(
            await home.harness.snapshot(LimitsDoc, hamlet, context),
        );
        return reply(
            message.id,
            limits.permissions[command.permission].includes(command.agent)
                ? `Revoked what was granted. \`${command.agent}\` still has ${command.permission} under [permissions] in limits.toml.`
                : `\`${command.agent}\` no longer has ${command.permission}.`,
        );
    }
    if (
        command?.command === "folder" ||
        command?.command === "mode" ||
        command?.command === "give"
    ) {
        try {
            const text =
                command.command === "folder"
                    ? await folder(
                          home,
                          message.to.hamlet,
                          command.path,
                          context,
                      )
                    : command.command === "mode"
                      ? await setMode(
                            home,
                            message.to.hamlet,
                            command.mode,
                            context,
                        )
                      : await giftSkill(
                            home,
                            { hamlet: message.to.hamlet, agent: command.agent },
                            command.skill,
                            command.from === undefined
                                ? undefined
                                : {
                                      hamlet: message.to.hamlet,
                                      agent: command.from,
                                  },
                            context,
                        );
            return reply(message.id, text);
        } catch (error) {
            return reply(
                message.id,
                safeWords(
                    home,
                    error instanceof Error ? error.message : String(error),
                ),
                "error",
            );
        }
    }
    if (command?.command === "stop") {
        const target =
            command.agent === null
                ? undefined
                : await addressed(
                      home.harness,
                      { hamlet: message.to.hamlet, agent: command.agent },
                      context,
                  );
        if (command.agent === null || target !== undefined)
            await stopAgent(home, target, context);
        return reply(
            message.id,
            command.agent === null
                ? "Stopping everything."
                : `Stopping ${command.agent}.`,
        );
    }
    const agent = await addressed(home.harness, message.to, context);
    if (
        agent !== undefined &&
        (command?.command === "new" ||
            command?.command === "resume" ||
            command?.command === "reflect" ||
            command?.command === "retry")
    ) {
        const chat = { agent, chief: home.chief };
        try {
            switch (command.command) {
                case "new":
                    await newChat(home, chat, context);
                    return reply(message.id, "Started a new session.");
                case "resume":
                    if (command.number === null) {
                        const past = (
                            await historyOf(home, chat, context)
                        ).filter((each) => !each.current);
                        return reply(
                            message.id,
                            past.length === 0
                                ? "There are no past sessions here."
                                : past
                                      .map(
                                          (each) =>
                                              `${each.number}: ${each.title} (${each.messages} messages, ${each.tokens} tokens; ${new Date(each.began).toISOString()})`,
                                      )
                                      .join("\n"),
                        );
                    }
                    if (
                        !(
                            await home.harness.snapshot(
                                ChatDoc,
                                agentKey(agent),
                                context,
                            )
                        )?.chats[home.chief]?.conversations.some(
                            (each) => each.number === command.number,
                        )
                    )
                        return reply(
                            message.id,
                            `There is no past session \`${command.number}\` here.`,
                        );
                    await resumeChat(home, chat, command.number, context);
                    return reply(
                        message.id,
                        "Went back to an earlier session.",
                    );
                case "reflect":
                    {
                        const reflection = await reflect(home, chat, context);
                        if (reflection !== undefined) submitted?.(reflection);
                    }
                    return undefined;
                case "retry":
                    if (
                        !(
                            await home.harness.snapshot(
                                ChatDoc,
                                agentKey(agent),
                                context,
                            )
                        )?.chats[home.chief]
                    )
                        return reply(
                            message.id,
                            "the last message was answered, so there is nothing to carry on",
                        );
                    {
                        const continued = await retry(home, chat, context);
                        submitted?.(continued);
                    }
                    return undefined;
            }
        } catch (error) {
            return reply(
                message.id,
                safeWords(
                    home,
                    error instanceof Error ? error.message : String(error),
                ),
            );
        }
    }
    const waits = queue(await home.harness.snapshot(OversightDoc, context));
    const chat =
        agent === undefined
            ? undefined
            : (await home.harness.snapshot(ChatDoc, agentKey(agent), context))
                  ?.chats[home.chief];
    const own = waits.find(
        (wait) =>
            (wait.by === message.to.agent &&
                (wait.agent === null ||
                    wait.agent.hamlet === message.to.hamlet)) ||
            (agent !== undefined &&
                wait.chat?.hamlet === agent.hamlet &&
                wait.chat.number === agent.number &&
                wait.chat.chief === home.chief &&
                wait.chat.chat === chat?.current),
    );
    if (command?.command === "queue")
        return reply(
            message.id,
            waits.length === 0
                ? "Nothing waits for you."
                : waits
                      .map(
                          (wait) =>
                              `${strip(wait)} (waited ${Math.floor(Math.max(0, home.now() - wait.since) / 1000)}s, asked ${wait.asked})`,
                      )
                      .join("\n"),
        );
    if (command?.command === "answer") {
        const wait = own ?? waits[0];
        if (wait === undefined)
            return reply(message.id, "Nothing waits for you.");
        try {
            return reply(
                message.id,
                await answerWords(
                    home,
                    {
                        wait: wait.id,
                        choice: command.choice,
                        text: command.text,
                        chief: home.chief,
                    },
                    context,
                ),
            );
        } catch (error) {
            return reply(
                message.id,
                safeWords(
                    home,
                    error instanceof Error ? error.message : String(error),
                ),
            );
        }
    }
    if (
        !text.trimStart().startsWith("/") &&
        own !== undefined &&
        (own.kind === "approval" || own.kind === "objection")
    )
        return reply(
            message.id,
            `Not sent. ${own.by} is waiting for your answer:\n${strip(own)}`,
        );
    if (agent === undefined) {
        throw new Error(`no agent named \`${message.to.agent}\``);
    }
    const submission = await submitLine(
        home,
        { agent, chief: home.chief },
        { id: message.id, text },
        context,
    );
    submitted?.(submission);
    return undefined;
}

function reply(
    to: string,
    text: string,
    kind: "notice" | "error" = "notice",
): Reply {
    return { type: "reply", to, kind, text };
}
