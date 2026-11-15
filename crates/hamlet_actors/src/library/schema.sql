-- The library's store: what git cannot hold. `marker` is the
-- repository module's (`repo/store.rs`), which makes it.

-- offers.rs: one row per offer, from its first part.
CREATE TABLE IF NOT EXISTS offer (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    agent INTEGER NOT NULL,
    call TEXT NOT NULL,
    skill TEXT NOT NULL,
    reason TEXT NOT NULL,
    -- NULL until its last part; then the commit `offers/<id>` names.
    commit_id TEXT,
    -- NULL until judged; then 1 or 0, with the librarian's words.
    accepted INTEGER,
    words TEXT,
    UNIQUE (agent, call)
) STRICT;

-- offers.rs: the pieces of an offer's files, kept until its last
-- part commits them.
CREATE TABLE IF NOT EXISTS offer_piece (
    offer INTEGER NOT NULL,
    path TEXT NOT NULL,
    offset INTEGER NOT NULL,
    bytes BLOB NOT NULL,
    last INTEGER NOT NULL,
    executable INTEGER NOT NULL,
    PRIMARY KEY (offer, path, offset)
) STRICT;

-- What happened on the shelves.
CREATE TABLE IF NOT EXISTS shelf_event (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- shelved, refused or unshelved
    kind TEXT NOT NULL,
    skill TEXT NOT NULL,
    -- As a commit names its author: `agent:<n>` or `chief:<n>`.
    by TEXT NOT NULL,
    -- The offer's reason, the librarian's words, or the chief's
    -- reason, empty for none.
    why TEXT NOT NULL,
    -- The commit of `main` it made; NULL for a refusal.
    commit_id TEXT,
    at INTEGER NOT NULL
) STRICT;

-- The agents that asked for the shelves, each told when they change.
CREATE TABLE IF NOT EXISTS listener (
    agent INTEGER PRIMARY KEY
) STRICT;
