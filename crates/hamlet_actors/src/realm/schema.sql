-- chiefs.rs
CREATE TABLE IF NOT EXISTS realm (
	id INTEGER PRIMARY KEY CHECK (id = 1),
	schema INTEGER NOT NULL,
	limits TEXT NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS chief (
	id INTEGER PRIMARY KEY CHECK (id = 1),
	name TEXT NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS hamlet (
	id TEXT PRIMARY KEY,
	name TEXT NOT NULL,
	mayor INTEGER NOT NULL,
	tier TEXT NOT NULL,
	folder TEXT,
	mode TEXT NOT NULL,
	limits TEXT NOT NULL,
	x REAL,
	z REAL,
	at INTEGER NOT NULL,
	-- 'living', 'removed' or 'erased'
	status TEXT NOT NULL DEFAULT 'living') STRICT;
CREATE TABLE IF NOT EXISTS hamlet_chief (
	hamlet TEXT NOT NULL,
	chief INTEGER NOT NULL,
	PRIMARY KEY (hamlet, chief)) STRICT;
-- chiefs/hamlet.rs
CREATE TABLE IF NOT EXISTS hamlet_folder (
	hamlet TEXT NOT NULL,
	folder TEXT,
	at INTEGER NOT NULL) STRICT;

-- profiles.rs
CREATE TABLE IF NOT EXISTS profile (
	id INTEGER PRIMARY KEY,
	kind TEXT NOT NULL,
	name TEXT NOT NULL,
	-- 'making' while a fork is made, 'living', 'archived' or 'erased'
	status TEXT NOT NULL DEFAULT 'living',
	-- 'harbor', 'housed' or 'turned'
	arrival TEXT NOT NULL DEFAULT 'housed',
	house_x REAL,
	house_z REAL,
	house_turn REAL,
	-- when it was made, and who made, housed or turned it away last:
	-- `chief` or `agent/<n>`
	since INTEGER NOT NULL DEFAULT 0,
	arrival_by TEXT,
	-- a fork's source profile and the commit it was made at
	fork_of INTEGER,
	fork_at TEXT) STRICT;

-- tree.rs
CREATE TABLE IF NOT EXISTS agent (
	id INTEGER PRIMARY KEY,
	kind TEXT NOT NULL,
	status TEXT NOT NULL,
	ended_how TEXT,
	ended_why TEXT,
	at INTEGER NOT NULL,
	erased INTEGER NOT NULL DEFAULT 0) STRICT;
CREATE TABLE IF NOT EXISTS hire (
	agent INTEGER PRIMARY KEY,
	hamlet TEXT NOT NULL,
	profile INTEGER NOT NULL,
	lead INTEGER,
	purpose TEXT,
	version TEXT) STRICT;
-- tree/tools.rs
CREATE TABLE IF NOT EXISTS hire_tool_off (
	hire INTEGER NOT NULL,
	tool TEXT NOT NULL,
	PRIMARY KEY (hire, tool)) STRICT;
-- tree/choices.rs
CREATE TABLE IF NOT EXISTS nickname (
	agent INTEGER PRIMARY KEY,
	name TEXT NOT NULL,
	by TEXT NOT NULL,
	at INTEGER NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS model_choice (
	of TEXT PRIMARY KEY,
	model TEXT,
	model_by TEXT,
	effort TEXT,
	fast INTEGER,
	at INTEGER NOT NULL,
	CHECK ((model IS NULL) = (model_by IS NULL))) STRICT;

-- versions.rs
CREATE TABLE IF NOT EXISTS reach (
	agent INTEGER PRIMARY KEY,
	wanted INTEGER NOT NULL,
	applied INTEGER) STRICT;
CREATE TABLE IF NOT EXISTS place (
	agent INTEGER PRIMARY KEY,
	version INTEGER NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS revision (
	id INTEGER PRIMARY KEY CHECK (id = 1),
	n INTEGER NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS copy (
	part TEXT PRIMARY KEY,
	version INTEGER NOT NULL) STRICT;

-- marker.rs
CREATE TABLE IF NOT EXISTS marker (
	caller TEXT NOT NULL,
	id TEXT NOT NULL,
	action TEXT NOT NULL,
	digest BLOB NOT NULL,
	done BLOB NOT NULL,
	pushes BLOB NOT NULL,
	at INTEGER NOT NULL,
	PRIMARY KEY (caller, id)) STRICT;

-- record.rs
CREATE TABLE IF NOT EXISTS decision (
	id INTEGER PRIMARY KEY,
	kind TEXT NOT NULL,
	hamlet TEXT,
	agent INTEGER,
	other INTEGER,
	tier TEXT,
	by TEXT NOT NULL,
	chief TEXT,
	why TEXT,
	detail BLOB,
	at INTEGER NOT NULL) STRICT;

-- steps.rs
CREATE TABLE IF NOT EXISTS step (
	kind TEXT NOT NULL,
	n INTEGER NOT NULL,
	done TEXT,
	closed INTEGER NOT NULL DEFAULT 0,
	at INTEGER NOT NULL,
	PRIMARY KEY (kind, n)) STRICT;

-- tasks.rs
CREATE TABLE IF NOT EXISTS task (
	id INTEGER PRIMARY KEY,
	origin TEXT NOT NULL,
	agent INTEGER,
	payer TEXT NOT NULL,
	text TEXT,
	at INTEGER NOT NULL) STRICT;

-- letters.rs
CREATE TABLE IF NOT EXISTS letter (
	id INTEGER PRIMARY KEY,
	kind TEXT NOT NULL,
	sender INTEGER NOT NULL,
	receiver INTEGER NOT NULL,
	way TEXT NOT NULL,
	text TEXT NOT NULL,
	task INTEGER,
	conversation TEXT,
	answer TEXT,
	given_at INTEGER,
	at INTEGER NOT NULL) STRICT;

-- laid.rs
CREATE TABLE IF NOT EXISTS road (
	a INTEGER NOT NULL,
	b INTEGER NOT NULL,
	hamlet TEXT NOT NULL,
	used_at INTEGER,
	up_at INTEGER,
	kind TEXT NOT NULL DEFAULT 'tree',
	at INTEGER NOT NULL,
	PRIMARY KEY (a, b),
	CHECK (a < b)) STRICT;

-- budget.rs
CREATE TABLE IF NOT EXISTS charge (
	agent INTEGER NOT NULL,
	call TEXT NOT NULL,
	payer TEXT NOT NULL,
	micros INTEGER NOT NULL,
	took_ms INTEGER NOT NULL,
	tokens INTEGER,
	at INTEGER NOT NULL,
	PRIMARY KEY (agent, call)) STRICT;
CREATE TABLE IF NOT EXISTS budget_reset (
	id INTEGER PRIMARY KEY,
	payer TEXT NOT NULL,
	last_charge INTEGER NOT NULL,
	at INTEGER NOT NULL) STRICT;

-- waits.rs
CREATE TABLE IF NOT EXISTS wait (
	id INTEGER PRIMARY KEY,
	agent INTEGER NOT NULL,
	kind TEXT NOT NULL DEFAULT 'tool',
	task INTEGER,
	conversation TEXT,
	call TEXT,
	subject TEXT,
	needs TEXT,
	reach_version INTEGER,
	detail BLOB,
	ruling TEXT,
	choice TEXT,
	scope TEXT,
	reason TEXT,
	closed_at INTEGER,
	at INTEGER NOT NULL,
	UNIQUE (agent, call),
	CHECK (kind <> 'tool' OR (call IS NOT NULL AND subject IS NOT NULL
		AND needs IS NOT NULL AND reach_version IS NOT NULL))) STRICT;

-- oversight.rs
CREATE TABLE IF NOT EXISTS grant (
	id INTEGER PRIMARY KEY,
	agent INTEGER NOT NULL,
	permission TEXT NOT NULL,
	at INTEGER NOT NULL,
	revoked_at INTEGER) STRICT;
CREATE TABLE IF NOT EXISTS trust (
	agent INTEGER NOT NULL,
	conversation TEXT NOT NULL,
	edits INTEGER NOT NULL DEFAULT 0,
	commands INTEGER NOT NULL DEFAULT 0,
	at INTEGER NOT NULL,
	PRIMARY KEY (agent, conversation)) STRICT;

-- oversight/asking.rs
CREATE TABLE IF NOT EXISTS objection (
	task INTEGER PRIMARY KEY,
	agent INTEGER NOT NULL,
	wait INTEGER NOT NULL,
	source TEXT NOT NULL,
	ruling TEXT,
	at INTEGER NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS held (
	letter INTEGER NOT NULL,
	what TEXT NOT NULL,
	task INTEGER NOT NULL,
	at INTEGER NOT NULL,
	PRIMARY KEY (letter, what)) STRICT;

-- workers.rs
CREATE TABLE IF NOT EXISTS worker (
	agent INTEGER PRIMARY KEY,
	starter INTEGER,
	hamlet TEXT,
	brief TEXT NOT NULL,
	tools TEXT NOT NULL,
	task INTEGER NOT NULL,
	conversation TEXT,
	ended TEXT,
	answer TEXT,
	at INTEGER NOT NULL) STRICT;

-- workers/errands.rs
CREATE TABLE IF NOT EXISTS errand (
	n INTEGER PRIMARY KEY,
	purpose TEXT NOT NULL,
	task INTEGER NOT NULL,
	cap INTEGER NOT NULL,
	subject INTEGER,
	instructions TEXT,
	text TEXT NOT NULL,
	model TEXT,
	ended TEXT,
	answer TEXT,
	at INTEGER NOT NULL) STRICT;

-- reviews.rs
CREATE TABLE IF NOT EXISTS review (
	task INTEGER PRIMARY KEY,
	hire INTEGER NOT NULL,
	lead INTEGER NOT NULL,
	line BLOB NOT NULL,
	outcome TEXT,
	at INTEGER NOT NULL) STRICT;

-- allowance.rs
CREATE TABLE IF NOT EXISTS allowance (
	hire INTEGER PRIMARY KEY,
	micros INTEGER NOT NULL,
	why TEXT,
	by TEXT NOT NULL,
	at INTEGER NOT NULL) STRICT;

-- society.rs
CREATE TABLE IF NOT EXISTS agreement (
	a INTEGER NOT NULL,
	b INTEGER NOT NULL,
	by INTEGER NOT NULL,
	at INTEGER NOT NULL,
	PRIMARY KEY (a, b, by),
	CHECK (a < b)) STRICT;

-- documents.rs
CREATE TABLE IF NOT EXISTS document (
	name TEXT PRIMARY KEY,
	value BLOB NOT NULL) STRICT;

-- settings.rs
CREATE TABLE IF NOT EXISTS settings (
	id INTEGER PRIMARY KEY CHECK (id = 1),
	value BLOB NOT NULL) STRICT;

-- parcels.rs
CREATE TABLE IF NOT EXISTS parcel (
	id INTEGER PRIMARY KEY,
	-- the letter it sends, and its sender: none for a chief's line
	letter INTEGER,
	sender INTEGER,
	receiver INTEGER NOT NULL,
	paths TEXT NOT NULL,
	state TEXT NOT NULL,
	swept_at INTEGER,
	at INTEGER NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS parcel_file (
	parcel INTEGER NOT NULL,
	file INTEGER NOT NULL,
	path TEXT NOT NULL,
	name TEXT NOT NULL,
	size INTEGER NOT NULL,
	sha256 BLOB NOT NULL,
	PRIMARY KEY (parcel, file)) STRICT;

-- dreams.rs
CREATE TABLE IF NOT EXISTS dream (
	id INTEGER PRIMARY KEY,
	profile INTEGER NOT NULL,
	hire INTEGER,
	at INTEGER NOT NULL,
	errand INTEGER,
	main_folder TEXT,
	hire_folder TEXT,
	done INTEGER,
	summary TEXT) STRICT;

-- newcomers.rs: the profile each of the Hand's errands judges
CREATE TABLE IF NOT EXISTS newcomer (
	errand INTEGER PRIMARY KEY,
	profile INTEGER NOT NULL) STRICT;

-- librarian.rs
CREATE TABLE IF NOT EXISTS librarian (
	errand INTEGER PRIMARY KEY,
	asker INTEGER NOT NULL,
	call TEXT NOT NULL,
	conversation TEXT,
	offer INTEGER,
	skill TEXT,
	accepted INTEGER,
	words TEXT) STRICT;

-- merges.rs
CREATE TABLE IF NOT EXISTS merge (
	id INTEGER PRIMARY KEY,
	absorbed INTEGER NOT NULL,
	kept INTEGER NOT NULL,
	-- the ids the profile's calls carry, for the merge and its split
	request TEXT NOT NULL,
	split_request TEXT NOT NULL,
	message TEXT NOT NULL,
	-- the merge's commit, once the profile made it
	tip TEXT,
	-- what it moved in the tree, as JSON, given back if it fails
	moved TEXT NOT NULL,
	split INTEGER NOT NULL DEFAULT 0,
	at INTEGER NOT NULL) STRICT;
