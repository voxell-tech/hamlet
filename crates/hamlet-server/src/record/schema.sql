CREATE TABLE record_hamlet (
    id TEXT PRIMARY KEY,
    document TEXT NOT NULL CHECK (json_valid(document))
) STRICT, WITHOUT ROWID;
CREATE TABLE record_name (
    name TEXT PRIMARY KEY,
    hamlet TEXT NOT NULL REFERENCES record_hamlet(id),
    number INTEGER NOT NULL CHECK (number >= 0),
    UNIQUE (hamlet, number)
) STRICT, WITHOUT ROWID;
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
