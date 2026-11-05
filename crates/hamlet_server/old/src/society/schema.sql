-- Each agent's conversations with the agents who write or ask it,
-- by the sender.
CREATE TABLE society_peer (
    hamlet TEXT NOT NULL,
    number INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('letters', 'questions')),
    from_hamlet TEXT NOT NULL,
    from_number INTEGER NOT NULL,
    binding INTEGER NOT NULL UNIQUE REFERENCES harness_binding(id),
    PRIMARY KEY (hamlet, number, kind, from_hamlet, from_number)
) STRICT, WITHOUT ROWID;
-- Who pays for a conversation's model work when it is not its own
-- hamlet, and what an errand is for.
CREATE TABLE society_binding (
    binding INTEGER PRIMARY KEY REFERENCES harness_binding(id),
    payer TEXT,
    -- An errand's kind: hearing, ballot, notes, librarian or review.
    errand TEXT,
    -- What the errand answers, as its owner reads it.
    owner TEXT CHECK (owner IS NULL OR json_valid(owner)),
    -- The prompt an errand runs with in place of the agent's.
    instructions TEXT
) STRICT;
-- A letter or a question carried along a road, and its answer.
CREATE TABLE society_message (
    -- The request id of its input: `letter:<n>` or `ask:<n>`.
    request TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('letter', 'question')),
    -- The tool call that sent it, under its operation identity.
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    -- The sender's conversation, which a letter's reply comes to.
    sender INTEGER NOT NULL REFERENCES harness_binding(id),
    receiver INTEGER NOT NULL REFERENCES harness_binding(id),
    from_hamlet TEXT NOT NULL,
    from_number INTEGER NOT NULL,
    to_hamlet TEXT NOT NULL,
    to_number INTEGER NOT NULL,
    address TEXT NOT NULL,
    task TEXT NOT NULL,
    text TEXT NOT NULL,
    at INTEGER NOT NULL,
    -- The sender's input whose turn asked it.
    input TEXT,
    -- Set once its input ended: the answer, or why it could not.
    answer TEXT,
    failure TEXT,
    -- Whether the way back was counted on its road.
    carried INTEGER NOT NULL DEFAULT 0 CHECK (carried IN (0, 1)),
    -- Whether a letter's reply was submitted to its sender.
    replied INTEGER NOT NULL DEFAULT 0 CHECK (replied IN (0, 1)),
    -- Whether a refused road ask was recorded.
    refused INTEGER NOT NULL DEFAULT 0 CHECK (refused IN (0, 1)),
    UNIQUE (runtime, key)
) STRICT;
-- The messages between agents each H12 task has used.
CREATE TABLE society_task (
    task TEXT PRIMARY KEY,
    messages INTEGER NOT NULL DEFAULT 0
) STRICT, WITHOUT ROWID;
-- A society tool call that waits, kept until it ends.
CREATE TABLE society_call (
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    binding INTEGER NOT NULL REFERENCES harness_binding(id),
    tool TEXT NOT NULL,
    -- The input whose turn made the call.
    input TEXT,
    data TEXT NOT NULL CHECK (json_valid(data)),
    PRIMARY KEY (runtime, key)
) STRICT, WITHOUT ROWID;
-- A society document kept whole, by its kind and key: the realm's
-- places and agreements, each hamlet's votes, the library's shelf.
CREATE TABLE society_document (
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL CHECK (json_valid(value)),
    PRIMARY KEY (kind, key)
) STRICT, WITHOUT ROWID;
