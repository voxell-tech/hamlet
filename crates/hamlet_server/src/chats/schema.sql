-- Each kept entry of a conversation, ordered by the runtime's entry id.
CREATE TABLE harness_item (
    binding INTEGER NOT NULL REFERENCES harness_binding(id),
    entry TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    item TEXT NOT NULL CHECK (json_valid(item)),
    PRIMARY KEY (binding, entry),
    UNIQUE (binding, ordinal)
) STRICT;
-- A chief's chat with an agent and the session in use.
CREATE TABLE chat (
    hamlet TEXT NOT NULL,
    number INTEGER NOT NULL,
    chief TEXT NOT NULL,
    current INTEGER NOT NULL,
    -- Reflections reserved so far, which numbers the next one.
    reflections INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (hamlet, number, chief)
) STRICT, WITHOUT ROWID;
CREATE TABLE chat_session (
    hamlet TEXT NOT NULL,
    number INTEGER NOT NULL,
    chief TEXT NOT NULL,
    session INTEGER NOT NULL,
    binding INTEGER NOT NULL REFERENCES harness_binding(id),
    began INTEGER NOT NULL,
    PRIMARY KEY (hamlet, number, chief, session)
) STRICT, WITHOUT ROWID;
-- What the server keeps of each conversation of an agent.
CREATE TABLE chat_conversation (
    binding INTEGER PRIMARY KEY REFERENCES harness_binding(id),
    hamlet TEXT NOT NULL,
    number INTEGER NOT NULL,
    chief TEXT,
    session INTEGER NOT NULL,
    -- The conversation a rewind put aside for this one.
    previous INTEGER REFERENCES harness_binding(id),
    -- Answered chief turns since the last reflection.
    unreflected INTEGER NOT NULL DEFAULT 0,
    retiring INTEGER NOT NULL DEFAULT 0 CHECK (retiring IN (0, 1)),
    -- The reflection input last entered.
    entered TEXT,
    -- How the last followed input ended, and which it was.
    last TEXT CHECK (last IS NULL OR last IN ('done', 'failed', 'stopped')),
    last_input TEXT,
    -- The budget words shown through the turn of `budget_input`.
    budget_input TEXT,
    budget_words TEXT
) STRICT;
-- Reflections reserved before they are submitted.
CREATE TABLE chat_reflection (
    binding INTEGER NOT NULL REFERENCES harness_binding(id),
    request TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    count INTEGER NOT NULL,
    task TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (binding, request)
) STRICT, WITHOUT ROWID;
-- A chief's instructions to each agent, the latest twenty.
CREATE TABLE chat_instruction (
    hamlet TEXT NOT NULL,
    number INTEGER NOT NULL,
    task TEXT NOT NULL,
    chief TEXT NOT NULL,
    text TEXT NOT NULL,
    at INTEGER NOT NULL,
    done INTEGER,
    PRIMARY KEY (hamlet, number, task)
) STRICT, WITHOUT ROWID;
-- A final answer that failed the server's check, by the input of its turn.
CREATE TABLE chat_answer (
    binding INTEGER NOT NULL REFERENCES harness_binding(id),
    input TEXT NOT NULL,
    words TEXT NOT NULL,
    reported INTEGER NOT NULL DEFAULT 0 CHECK (reported IN (0, 1)),
    PRIMARY KEY (binding, input)
) STRICT, WITHOUT ROWID;
-- What waits for a chief, open or ended.
CREATE TABLE wait (
    id TEXT PRIMARY KEY,
    ordinal INTEGER NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('approval', 'fault', 'question', 'objection')),
    danger TEXT,
    by TEXT NOT NULL,
    hamlet TEXT,
    number INTEGER,
    about TEXT NOT NULL,
    change TEXT CHECK (change IS NULL OR json_valid(change)),
    answers TEXT NOT NULL CHECK (json_valid(answers)),
    -- The normalized questions and allowed answers it asks.
    questions TEXT,
    since INTEGER NOT NULL,
    asked INTEGER NOT NULL,
    -- The chief's chat it was raised in.
    chat_hamlet TEXT,
    chat_number INTEGER,
    chat_chief TEXT,
    chat_session INTEGER,
    open INTEGER NOT NULL DEFAULT 1 CHECK (open IN (0, 1)),
    how TEXT,
    text TEXT,
    chief TEXT,
    settled INTEGER
) STRICT, WITHOUT ROWID;
-- The gate's decision of each tool call, kept by its operation identity.
CREATE TABLE gate_call (
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    binding INTEGER NOT NULL REFERENCES harness_binding(id),
    -- The exact request the identity authorizes.
    digest BLOB NOT NULL,
    -- The runtime's submission id of the input whose turn made it.
    input TEXT,
    -- The reach it runs with once allowed: whether it has the network.
    net INTEGER CHECK (net IS NULL OR net IN (0, 1)),
    block TEXT,
    -- The wait that holds it, and how that wait ended.
    wait TEXT REFERENCES wait(id),
    how TEXT,
    text TEXT,
    change TEXT CHECK (change IS NULL OR json_valid(change)),
    PRIMARY KEY (runtime, key)
) STRICT, WITHOUT ROWID;
-- The refusals and questions of a conversation's current turn.
CREATE TABLE gate_asked (
    binding INTEGER PRIMARY KEY REFERENCES harness_binding(id),
    input TEXT,
    strikes INTEGER NOT NULL DEFAULT 0,
    questions INTEGER NOT NULL DEFAULT 0,
    refused TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(refused))
) STRICT;
-- Edits and commands trusted in one numbered session of a chat.
CREATE TABLE gate_trust (
    hamlet TEXT NOT NULL,
    number INTEGER NOT NULL,
    chief TEXT NOT NULL,
    session INTEGER NOT NULL,
    edit INTEGER NOT NULL DEFAULT 0 CHECK (edit IN (0, 1)),
    command INTEGER NOT NULL DEFAULT 0 CHECK (command IN (0, 1)),
    PRIMARY KEY (hamlet, number, chief, session)
) STRICT, WITHOUT ROWID;
-- Lasting permissions given and taken back, in order.
CREATE TABLE grant_event (
    ordinal INTEGER PRIMARY KEY,
    type TEXT NOT NULL CHECK (type IN ('granted', 'revoked')),
    hamlet TEXT NOT NULL,
    number INTEGER NOT NULL,
    name TEXT NOT NULL,
    permission TEXT NOT NULL CHECK (permission IN ('network', 'unconfined')),
    by TEXT NOT NULL,
    at INTEGER NOT NULL
) STRICT;
