//! Durable sends. An actor writes a send into its own file, in the
//! same transaction as the change that caused it, and the host
//! delivers it until the receiver confirms it, once.

/// The send tables of an actor file, and the file's own id, which its
/// first start makes and no later start changes. The start
/// transaction runs it before the core sees the file.
///
/// - `_hamlet_file`: the id of this file. A key destroyed and made
///   again is a new file with a new id, so a receiver tells the lives
///   of one sender key apart.
/// - `_hamlet_outbox`: sends this actor made that their receiver has
///   not confirmed, numbered from 1 per receiver.
/// - `_hamlet_peers`: per receiver, the next number to give and the
///   highest number it confirmed.
/// - `_hamlet_inbound`: per sender life, the highest number handled
///   here, the runs of the next one that ended without success, and
///   the generation of this file whose run of it is under way.
/// - `_hamlet_handled`: the answer to each send handled here, kept
///   until its sender has confirmed it.
/// - `_hamlet_dead_letters`: sends set aside, with the reason.
pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS _hamlet_file (
	id INTEGER PRIMARY KEY CHECK (id = 0),
	file_id TEXT NOT NULL) STRICT;
INSERT OR IGNORE INTO _hamlet_file (id, file_id)
	VALUES (0, lower(hex(randomblob(16))));
CREATE TABLE IF NOT EXISTS _hamlet_outbox (
	to_kind TEXT NOT NULL,
	to_key TEXT NOT NULL,
	seq INTEGER NOT NULL,
	name TEXT NOT NULL,
	payload BLOB NOT NULL,
	task TEXT NOT NULL,
	cause TEXT,
	created_at INTEGER NOT NULL,
	PRIMARY KEY (to_kind, to_key, seq)) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS _hamlet_peers (
	kind TEXT NOT NULL,
	key TEXT NOT NULL,
	next_out INTEGER NOT NULL,
	acked_out INTEGER NOT NULL,
	PRIMARY KEY (kind, key)) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS _hamlet_inbound (
	kind TEXT NOT NULL,
	key TEXT NOT NULL,
	file_id TEXT NOT NULL,
	handled_in INTEGER NOT NULL,
	attempts_in INTEGER NOT NULL,
	running_in INTEGER,
	PRIMARY KEY (kind, key, file_id)) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS _hamlet_handled (
	kind TEXT NOT NULL,
	key TEXT NOT NULL,
	file_id TEXT NOT NULL,
	seq INTEGER NOT NULL,
	outcome BLOB NOT NULL,
	PRIMARY KEY (kind, key, file_id, seq)) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS _hamlet_dead_letters (
	id INTEGER PRIMARY KEY,
	peer_kind TEXT NOT NULL,
	peer_key TEXT NOT NULL,
	seq INTEGER,
	work_id TEXT,
	name TEXT NOT NULL,
	payload BLOB NOT NULL,
	task TEXT NOT NULL,
	cause TEXT,
	reason TEXT NOT NULL,
	attempts INTEGER NOT NULL,
	error TEXT,
	at INTEGER NOT NULL) STRICT;
";
