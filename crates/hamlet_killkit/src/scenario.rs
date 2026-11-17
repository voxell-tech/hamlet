//! The scenario table. A scenario is data: the lines the demo host
//! runs, when it is killed, how long it stays down, the lines each
//! restart runs, and what each restart must show. A new scenario is a
//! new row.

mod agent;
mod chat;
mod counter;
mod link;
mod workspace;

/// One way to kill the demo host and what must hold after it.
pub struct Scenario {
    pub name: &'static str,
    pub about: &'static str,
    pub demo: Demo,
    /// Lines `demo serve` runs before the kill. Each waits for its
    /// reply, which acknowledges it.
    pub setup: &'static [&'static str],
    /// The line the kill follows.
    pub trigger: &'static str,
    /// Whether the trigger replies before the kill, so the kill
    /// comes after its acknowledgement.
    pub trigger_replies: bool,
    /// When the kit kills the host.
    pub kill: Kill,
    /// How long after the trigger was sent the first restart starts.
    pub down_ms: u64,
    /// A due time, in Unix milliseconds, read once the host and its
    /// Node host are down. The first restart must start after it.
    pub due: Option<Probe>,
    /// Lines the first and the second restart's `demo serve` run
    /// after it boots. After [`Kill::NodeHost`] the first restart is
    /// the Rust host's own, of its Node host: the demo host that
    /// ran the trigger runs these lines.
    pub restart: [&'static [&'static str]; 2],
    /// Values read from the actor files once each restart exited.
    pub probes: &'static [Probe],
    /// The values the first and the second restart must show.
    pub expect: [&'static [Expect]; 2],
}

/// How the demo host is started.
pub enum Demo {
    /// `demo serve`: every kind in the one Rust host.
    Local,
    /// `demo --link serve`: the agents in a Node host, the Rust
    /// host's child. `pid` names the value that holds the Node
    /// host's process id, which a TypeScript actor answers. The kit
    /// checks that each Node host a run saw has ended once its demo
    /// host has, and that each start has a Node host of its own and
    /// no other child when that value comes.
    Linked { pid: &'static str },
}

/// When the kit kills the host, once the trigger was sent and, for a
/// trigger that replies, its reply came.
pub enum Kill {
    /// At a fault point, given as `HAMLET_FAULT` is:
    /// `<point>[:<pass>]`. The host stops there and is killed.
    Fault(&'static str),
    /// At a time drawn at random from `min_ms` to `max_ms`, both
    /// included. The run's line records the time drawn.
    After { min_ms: u64, max_ms: u64 },
    /// The trigger's own action kills the Node host it runs in, at a
    /// step the action names. The kit kills nothing: it waits for
    /// that Node host to end. The demo host stays up, and its
    /// supervisor starts the next Node host.
    NodeHost,
}

/// A value read from one actor file, under the realm folder.
pub struct Probe {
    pub name: &'static str,
    pub file: &'static str,
    pub read: Read,
}

/// How a probe reads its value.
pub enum Read {
    /// A query that returns one integer.
    Sql(&'static str),
    /// How many of the file, its write-ahead file and its
    /// shared-memory file exist.
    Files,
}

/// A value a restart must show: an actor's last reply, as
/// `counter/<key>`; the count of an actor its boot started, as
/// `boot counter/<key>`; how many commands failed with one code, as
/// `error <group.code>`; or a probe, by its name. A restart that
/// shows an error no expectation names fails.
pub struct Expect {
    pub value: &'static str,
    pub is: Is,
}

pub enum Is {
    /// The last count the actor acknowledged before the kill, plus
    /// this much.
    Acked(&'static str, i64),
    Exactly(i64),
    /// The restart showed it, with any value.
    Present,
    /// The restart did not show it.
    Absent,
}

pub fn all() -> impl Iterator<Item = &'static Scenario> {
    [
        counter::SCENARIOS,
        workspace::SCENARIOS,
        link::SCENARIOS,
        agent::SCENARIOS,
        chat::SCENARIOS,
    ]
    .into_iter()
    .flatten()
}
