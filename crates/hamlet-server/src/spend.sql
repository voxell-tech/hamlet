ALTER TABLE harness_attempt ADD COLUMN task TEXT;
ALTER TABLE harness_attempt ADD COLUMN price TEXT;
ALTER TABLE harness_usage ADD COLUMN ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE harness_usage ADD COLUMN estimated INTEGER NOT NULL DEFAULT 0;
ALTER TABLE harness_usage ADD COLUMN detail TEXT NOT NULL DEFAULT '{}';
ALTER TABLE harness_usage ADD COLUMN ordinal INTEGER NOT NULL DEFAULT 0;
UPDATE harness_usage SET ordinal = (SELECT n FROM (SELECT runtime, key, row_number() OVER (ORDER BY at, runtime, key) n FROM harness_usage) numbered WHERE numbered.runtime=harness_usage.runtime AND numbered.key=harness_usage.key);
CREATE UNIQUE INDEX usage_order ON harness_usage(ordinal);
CREATE TABLE spend_reset (
    id INTEGER PRIMARY KEY,
    hamlet TEXT NOT NULL,
    at INTEGER NOT NULL,
    by TEXT NOT NULL,
    models INTEGER NOT NULL,
    roads INTEGER NOT NULL
) STRICT;
