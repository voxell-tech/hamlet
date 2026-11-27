import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { Part, PlaceText } from "../src/agent/messages/realm.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    named,
    readPrompt,
    toolCall,
    toolResults,
    until,
    wakeAgent,
} from "./agent-testing.ts";
import {
    ANSWERED_BY_TOOLS,
    type Answers,
    answer,
    answering,
    answersTo,
    CALLED,
    HAND,
    LAST_WORDS,
    listed,
    NOT_WORDS,
    submitted,
    threw,
    turn,
} from "./messages-testing.ts";
import { WRITTEN } from "./messages-written.ts";
import { context, fixture } from "./testing.ts";

/**
 * A refusal the model is told in the words of a result, not of an error:
 * a call a chief must allow, and the chief that cannot be asked.
 */
function toldPlainly(given: {
    why?: { type: string; most?: number };
}): string | undefined {
    switch (given.why?.type) {
        case "reserved":
            return "This call has ended; the chief's answer comes to you as a new input.";
        case "no_chief":
            return "No chief oversees this turn; go on with what you know.";
        case "too_many":
            return `You have asked the chief ${given.why.most === 1 ? "once" : `${given.why.most} times`} this turn; go on with what you know.`;
        default:
            return undefined;
    }
}

for (const action of ANSWERED_BY_TOOLS) {
    test(`each answer to ${action} gives its tool a result, never a throw`, async (t) => {
        const row = WRITTEN.find(
            (written) =>
                written.fixture === action ||
                written.fixture.startsWith(`${action}.`),
        );
        const tool = row?.tool ?? CALLED[action];
        assert.ok(tool);
        const answers = answersTo(action).filter((name) => !listed(name));
        const queue = [...answers];
        const [folder = "", name = ""] = action.split("/");
        const { calls } = fakeCalls({
            ...row?.answers,
            [folder]: {
                ...row?.answers?.[folder as keyof Answers],
                [name]: () => fixture(queue.shift() as string),
            },
        });
        const agent = await turn(t, calls, {
            submit: submitted(row?.task),
            responses: answers.map((_, index) =>
                toolCall(tool[0], tool[1], `c${index}`),
            ),
            ...(row?.files === undefined ? {} : { files: row.files }),
            ...(row?.agent === undefined ? {} : { agent: row.agent }),
        });
        const results = await toolResults(agent);
        await agent.stop();

        assert.equal(results.length, answers.length);
        answers.forEach((name, index) => {
            const result = results[index];
            assert.ok(result && !threw(result), `${name} threw`);
            const given = fixture(name) as {
                type: string;
                why?: { type: string; most?: number };
                words?: string;
            };
            const plainly = toldPlainly(given);
            const told = [result.output, result.isError];
            if (plainly !== undefined) {
                assert.deepEqual(told, [plainly, false], name);
            } else if (given.type === "refused") {
                // A letter whose files it may not send is blocked.
                const words =
                    given.why?.type === "files"
                        ? `Tool call blocked: ${given.words}`
                        : given.words;
                assert.deepEqual(told, [words, true], name);
            } else {
                const shown = [result.output]
                    .concat(result.diagnostics.map(({ message }) => message))
                    .join("");
                assert.notEqual(shown, "", name);
                assert.doesNotMatch(shown, NOT_WORDS, name);
            }
        });
    });
}

for (const name of answersTo("profile/files")) {
    test(`${name} is read by a pull: a part is written, and a gone commit is pulled whole`, async (t) => {
        const given = fixture(name) as { type: string };
        const whole = answer("profile/files.part.last");
        const { calls, sent } = fakeCalls({
            profile: {
                files: answering(whole, given as never, whole, whole),
            },
        });
        const agent = await wakeAgent(t, { calls, input: HAND });
        await agent.actions.fill(agent.by(from.realm), { profile: 2 });
        await agent.actions.moved(agent.by(from.profile(2)), {
            tip: "cd".repeat(20),
        });
        const since = { since: "ab".repeat(20) };
        const expected =
            given.type === "refused"
                ? [{}, since, {}]
                : [
                      {},
                      since,
                      ...("next" in given
                          ? [
                                {
                                    ...since,
                                    after: (given as { next: unknown }).next,
                                },
                            ]
                          : []),
                  ];
        await until(() => named(sent, "files").length === expected.length);
        await agent.stop();

        assert.deepEqual(
            named(sent, "files").map((call) => call.message),
            expected,
        );
        assert.deepEqual(agent.reported, []);
    });
}

for (const name of answersTo("profile/save")) {
    test(`${name} is read by a save: a gone commit is filled again and saved once more on its tip, and another refusal is reported in its words`, async (t) => {
        const given = fixture(name) as {
            type: string;
            why?: { type: string };
            words?: string;
        };
        const gone = given.why?.type === "commit_gone";
        const files: Record<string, string> = {};
        const { calls, sent } = fakeCalls({
            profile: {
                files: () => answer("profile/files.part.last"),
                save: () => given as never,
            },
        });
        const agent = await wakeAgent(t, {
            calls,
            input: HAND,
            files: fakeFiles(files),
            responses: [fauxAssistantMessage(LAST_WORDS)],
        });
        await agent.actions.fill(agent.by(from.realm), { profile: 2 });
        files["memory/tests.md"] = "# Notes\n";
        await agent.actions.submit(agent.by(from.realm), submitted(undefined));
        await agent.idle();
        await agent.stop();

        assert.deepEqual(
            agent.reported,
            given.type === "refused"
                ? [`the filesystem was not saved: ${given.words}`]
                : [],
        );
        assert.equal(named(sent, "save").length, gone ? 2 : 1);
        assert.deepEqual(
            named(sent, "files").map((call) => call.message),
            given.type !== "refused"
                ? [{}, { since: "ab".repeat(20) }]
                : gone
                  ? [{}, {}]
                  : [{}],
        );
    });
}

for (const name of answersTo("library/offer")) {
    test(`${name} is read by submit_skill, as the answer to the first of its parts`, async (t) => {
        const given = fixture(name) as { type: string; words?: string };
        let offers = 0;
        const { calls, sent } = fakeCalls({
            library: {
                offer: () =>
                    offers++ === 0
                        ? (given as never)
                        : answer("library/offer.kept"),
            },
            realm: { submit_skill: () => answer("realm/submit_skill.done") },
        });
        const agent = await turn(t, calls, {
            submit: submitted(undefined),
            responses: [toolCall("submit_skill", { skill: "fixtures" })],
            files: {
                "skills/fixtures/SKILL.md": "---\nname: fixtures\n---\n",
                "skills/fixtures/large.md": "a".repeat(800 * 1024),
            },
        });
        const [result] = await toolResults(agent);
        await agent.stop();
        assert.ok(result && !threw(result), `${name} threw`);
        if (given.type === "refused") {
            assert.equal(result.output, given.words);
            assert.equal(named(sent, "offer").length, 1);
            assert.deepEqual(named(sent, "submit_skill"), []);
        } else {
            assert.equal(result.output, "The librarian is judging it.");
            assert.ok(named(sent, "offer").length > 1);
            assert.equal(named(sent, "submit_skill").length, 1);
        }
    });
}

for (const name of answersTo("realm/may_spend")) {
    test(`${name} is read by the model gate, and the charge's answer by its sender`, async (t) => {
        const given = fixture(name) as {
            type: string;
            why?: { type: string };
            words?: string;
        };
        const { calls } = fakeCalls({
            realm: {
                may_spend: () => given as never,
                charge: () => answer("realm/charge"),
            },
        });
        const agent = await turn(t, calls, { submit: submitted(undefined) });
        const harness = await agent.harness();
        const root = await harness.root(context);
        const submission = await harness.commit(
            (tx) => tx.submissionByRequest(root.id, "input-1"),
            context,
        );
        await agent.stop();

        if (given.type === "refused") {
            assert.equal(agent.faux.state.callCount, 0);
            const detail = String(submission?.detail);
            assert.ok(
                given.why?.type === "objection" ||
                    given.why?.type === "withdrawn"
                    ? detail === given.words
                    : detail.endsWith(`: ${given.words}`),
                detail,
            );
        } else {
            assert.equal(agent.faux.state.callCount, 1);
        }
        assert.deepEqual(agent.reported, []);
    });
}

for (const name of answersTo("realm/read_place")) {
    test(`${name}'s sections end the prompt, each that it holds`, async (t) => {
        const given = fixture(name) as PlaceText;
        const { calls } = fakeCalls({
            realm: { read_place: () => given },
        });
        const agent = await turn(t, calls, { submit: submitted(undefined) });
        const prompt = await readPrompt(agent);
        await agent.stop();

        const sections = [given.place, given.realm, given.profiles].filter(
            (text) => text !== undefined,
        );
        assert.ok(prompt.endsWith(`\n\n${sections.join("\n\n")}`), prompt);
    });
}

for (const name of answersTo("realm/fetch_part")) {
    test(`${name} is read by the parcel's saving`, async (t) => {
        const given = fixture(name) as Part;
        const last = fixture("realm/fetch_part.bytes.last.answer") as Part;
        const held: Record<string, string> = {};
        const { calls } = fakeCalls({
            realm: { fetch_part: ({ part }) => (part === 0 ? given : last) },
        });
        const agent = await wakeAgent(t, {
            calls,
            files: fakeFiles(held),
            responses: [fauxAssistantMessage(LAST_WORDS)],
        });
        await agent.actions.submit(agent.by(from.realm), {
            id: "input-1",
            input: {
                type: "letter",
                from: { agent: 12, name: "Ada" },
                text: "The report",
                files: [
                    {
                        parcel: 8,
                        file: 0,
                        path: "from-hire-12/report.md",
                        size: 2048,
                    },
                ],
            },
        });
        await agent.idle();
        await agent.stop();

        const text = (part: Part) =>
            part.type === "bytes"
                ? Buffer.from(part.bytes, "base64").toString()
                : "";
        if (given.type === "bytes") {
            assert.equal(
                held["inbox/from-hire-12/report.md"],
                text(given) + (given.last ? "" : text(last)),
            );
            assert.deepEqual(agent.reported, []);
        } else {
            assert.deepEqual(held, {});
            assert.deepEqual(agent.reported, [
                `from-hire-12/report.md was not saved in the inbox: ${given.words}`,
            ]);
        }
    });
}

for (const name of answersTo("realm/reply")) {
    test(`${name} is read by the reply`, async (t) => {
        const given = fixture(name) as { type: string; words?: string };
        const { calls } = fakeCalls({ realm: { reply: () => given as never } });
        const row = WRITTEN.find(
            (written) => written.fixture === "realm/reply",
        );
        const agent = await turn(t, calls, {
            submit: submitted(row?.task, row?.input),
        });
        await agent.stop();

        assert.deepEqual(
            agent.reported,
            given.type === "refused"
                ? [`a reply was refused: ${given.words}`]
                : [],
        );
    });
}
