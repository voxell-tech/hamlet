CREATE TABLE harness_runtime (
    id TEXT PRIMARY KEY,
    harness TEXT NOT NULL,
    -- As the runtime first reported them; a runtime's versions never change.
    adapter TEXT,
    version TEXT,
    -- Whether its store was made, so a missing one is an error.
    begun INTEGER NOT NULL DEFAULT 0 CHECK (begun IN (0, 1))
) STRICT, WITHOUT ROWID;
CREATE TABLE harness_agent (
    hamlet TEXT NOT NULL,
    number INTEGER NOT NULL CHECK (number >= 0),
    harness TEXT NOT NULL,
    PRIMARY KEY (hamlet, number)
) STRICT, WITHOUT ROWID;
CREATE TABLE harness_binding (
    id INTEGER PRIMARY KEY,
    hamlet TEXT NOT NULL,
    number INTEGER NOT NULL CHECK (number >= 0),
    errand INTEGER NOT NULL CHECK (errand IN (0, 1)),
    model TEXT NOT NULL CHECK (json_valid(model)),
    runtime TEXT NOT NULL REFERENCES harness_runtime(id),
    -- The runtime's id for the conversation and the versions it began with, once made.
    conversation TEXT,
    adapter TEXT,
    version TEXT,
    -- A cancel the runtime has not confirmed yet, sent again when it comes back.
    cancelling INTEGER NOT NULL DEFAULT 0 CHECK (cancelling IN (0, 1)),
    UNIQUE (runtime, conversation)
) STRICT;
CREATE TABLE harness_input (
    binding INTEGER NOT NULL REFERENCES harness_binding(id),
    request TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    content TEXT NOT NULL,
    steer INTEGER NOT NULL CHECK (steer IN (0, 1)),
    state TEXT NOT NULL CHECK (state IN ('kept', 'admitted', 'dropped')),
    PRIMARY KEY (binding, request),
    UNIQUE (binding, ordinal)
) STRICT;
CREATE TABLE harness_wait (
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    binding INTEGER NOT NULL REFERENCES harness_binding(id),
    tool TEXT NOT NULL,
    digest BLOB NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'allowed', 'refused')),
    words TEXT,
    PRIMARY KEY (runtime, key)
) STRICT, WITHOUT ROWID;
CREATE TABLE harness_attempt (
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    binding INTEGER NOT NULL REFERENCES harness_binding(id),
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (runtime, key)
) STRICT, WITHOUT ROWID;
CREATE TABLE harness_usage (
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    input INTEGER NOT NULL CHECK (input >= 0),
    output INTEGER NOT NULL CHECK (output >= 0),
    cache_read INTEGER NOT NULL CHECK (cache_read >= 0),
    cache_write INTEGER NOT NULL CHECK (cache_write >= 0),
    micros INTEGER NOT NULL CHECK (micros >= 0),
    at INTEGER NOT NULL,
    PRIMARY KEY (runtime, key),
    FOREIGN KEY (runtime, key) REFERENCES harness_attempt(runtime, key)
) STRICT, WITHOUT ROWID;
