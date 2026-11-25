-- The profile's store: what git cannot hold. `marker` is the
-- repository module's (`repo/store.rs`), which makes it.

-- hires.rs: one row per hire's branch.
CREATE TABLE IF NOT EXISTS hire (
    agent INTEGER PRIMARY KEY,
    branch TEXT NOT NULL UNIQUE,
    -- The version the hire is pinned to and the commit it resolved
    -- to; both NULL for a hire that follows `main`.
    pin TEXT,
    pin_commit TEXT,
    -- The character the agent is given: its version, and the blob of
    -- `CHARACTER.md` the version counts changes of, empty for none.
    character INTEGER NOT NULL,
    character_blob TEXT NOT NULL
) STRICT;

-- files.rs: the pieces of a save, kept until its last part commits
-- them.
CREATE TABLE IF NOT EXISTS save (
    agent INTEGER NOT NULL,
    call TEXT NOT NULL,
    path TEXT NOT NULL,
    offset INTEGER NOT NULL,
    -- NULL for a path the save removes.
    bytes BLOB,
    last INTEGER NOT NULL,
    executable INTEGER NOT NULL,
    PRIMARY KEY (agent, call, path, offset)
) STRICT;

-- files.rs
CREATE TABLE IF NOT EXISTS save_call (
    agent INTEGER PRIMARY KEY,
    call TEXT NOT NULL,
    next_part INTEGER NOT NULL
) STRICT;

-- dreams.rs
CREATE TABLE IF NOT EXISTS dream (
    id INTEGER PRIMARY KEY,
    -- NULL for a dream of the profile itself.
    hire INTEGER,
    -- `main` when the folders were written: the base its end merges
    -- from.
    main TEXT NOT NULL,
    -- The hire's branch when the folders were written: the base of
    -- the hire's side. NULL for a dream of the profile itself.
    hire_tip TEXT,
    -- NULL while it runs; then the CBOR of its `DreamEnd`.
    outcome BLOB
) STRICT;

-- merges.rs: the pieces of a merge from another profile, kept until
-- its last part merges them.
CREATE TABLE IF NOT EXISTS merge_part (
    id TEXT NOT NULL,
    path TEXT NOT NULL,
    offset INTEGER NOT NULL,
    -- NULL for a path the merge removes.
    bytes BLOB,
    last INTEGER NOT NULL,
    executable INTEGER NOT NULL,
    -- The blob a removed path held; NULL for a file.
    blob TEXT,
    PRIMARY KEY (id, path, offset)
) STRICT;

-- merges.rs
CREATE TABLE IF NOT EXISTS merge_call (
    id TEXT PRIMARY KEY,
    next_part INTEGER NOT NULL
) STRICT;

-- merges.rs: the commit of every merge, which a split may undo.
CREATE TABLE IF NOT EXISTS merge_commit (
    id TEXT PRIMARY KEY
) STRICT;
