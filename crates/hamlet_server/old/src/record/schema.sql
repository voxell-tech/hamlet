CREATE TABLE record_hamlet (
    id TEXT PRIMARY KEY,
    document TEXT NOT NULL CHECK (json_valid(document))
) STRICT, WITHOUT ROWID;
-- Rows are never deleted, so an id is never given twice.
CREATE TABLE record_scholar (
    id INTEGER PRIMARY KEY,
    name TEXT,
    chief TEXT NOT NULL,
    since INTEGER NOT NULL,
    left_at INTEGER,
    left_by TEXT,
    left_reason TEXT,
    archive TEXT,
    CHECK ((name IS NULL) = (left_at IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX record_scholar_name ON record_scholar(name)
    WHERE name IS NOT NULL;
CREATE INDEX record_scholar_chief ON record_scholar(chief);
CREATE TABLE record_scholar_seed (
    scholar INTEGER PRIMARY KEY REFERENCES record_scholar(id),
    character TEXT NOT NULL
) STRICT;
-- The living hires of the living hamlets, rebuilt from each hamlet's
-- document as it is kept.
CREATE TABLE record_hire (
    hamlet TEXT NOT NULL REFERENCES record_hamlet(id),
    number INTEGER NOT NULL CHECK (number >= 0),
    scholar INTEGER NOT NULL REFERENCES record_scholar(id),
    PRIMARY KEY (hamlet, number)
) STRICT, WITHOUT ROWID;
CREATE INDEX record_hire_scholar ON record_hire(scholar);
CREATE TABLE record_road_charge (
    id INTEGER PRIMARY KEY,
    hamlet TEXT NOT NULL REFERENCES record_hamlet(id),
    agent INTEGER NOT NULL,
    task TEXT,
    at INTEGER NOT NULL,
    tokens INTEGER NOT NULL CHECK (tokens >= 0),
    micros INTEGER NOT NULL CHECK (micros >= 0),
    ms INTEGER NOT NULL CHECK (ms >= 0)
) STRICT;
CREATE INDEX record_road_charge_period ON record_road_charge(hamlet, at);
