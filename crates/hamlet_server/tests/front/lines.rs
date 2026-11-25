//! A chief's line the door hands to the realm, and the stops that
//! withdraw it there: a line is the realm's once the door hands it
//! over, and a stop is one act of the realm.

use std::mem::ManuallyDrop;
use std::process::Command;
use std::time::{Duration, Instant};

use hamlet_acceptance::{Client, chat};
use hamlet_protocol::client::{
    AgentId, ClientMessage, ItemKind, ReplyKind, Stop, Transcript,
};
use hamlet_testing::{Scratch, Server};
use serde_json::json;

use super::{HeldNode, kill_node, said_once_hired, serve, until};

/// Lines to the mayor, each sent with a stop of the Hand right
/// behind it on the same connection.
const ROUNDS: usize = 5;

/// What the faux model answers each line the agent runs.
const ANSWER: &str = "Answered.";

/// A stop's bound with the agent's host frozen: the realm counts the
/// turns itself after 500 ms, and the rest is the door's two calls of
/// the realm, a few milliseconds each.
const FROZEN_BOUND: Duration = Duration::from_secs(1);

/// How long the Node host stays down while a line and `/new` wait:
/// the realm's tries of the line, 50 ms apart and doubling, are near
/// a second apart by then, and the door tries `/new` every 50 ms.
const DOWN_UNTIL_TRIES_SPREAD: Duration =
    Duration::from_millis(1_500);

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_of_one_agent_leaves_a_line_to_another_on_the_same_connection()
 {
    let answers = std::iter::once(json!("Here."))
        .chain((0..ROUNDS).map(|_| json!(ANSWER)))
        .collect::<Vec<_>>();
    let (mut server, _home, _side) = serve(json!(answers)).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    chief.command(hand, "/create-hamlet Docs").await;
    let founded = until(&server, "the hamlet", |realm| {
        !realm.hamlets.is_empty()
    })
    .await;
    let mayor = founded.hamlets[0].mayor;
    said_once_hired(&mut chief, mayor, "Are you hired?").await;
    for n in 0..ROUNDS {
        let line = chief.tell(mayor, &format!("Line {n}.")).await;
        let stop = chief
            .request(|id| {
                ClientMessage::Stop(Stop {
                    agent: Some(hand),
                    id,
                })
            })
            .await;
        assert_eq!(
            (stop.kind, stop.text.as_str()),
            (ReplyKind::Notice, "Nothing was running."),
            "round {n}"
        );
        let reply = chief.reply(&line).await;
        assert_eq!(
            (reply.kind, reply.text.as_str()),
            (ReplyKind::Notice, ""),
            "round {n}"
        );
    }
    let page = chief.open(&chat(mayor)).await;
    for n in 0..ROUNDS {
        assert_eq!(
            shown(&page, &format!("Line {n}.")),
            1,
            "{page:?}"
        );
    }
    assert_eq!(
        answers_in(&page).len(),
        ROUNDS + 1,
        "each line to the mayor ran: {page:?}"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// The Node host started again answers from the faux model's first
/// answer again, so each line that runs is answered [`ANSWER`].
#[tokio::test(flavor = "multi_thread")]
async fn a_line_a_stop_withdrew_is_answered_and_never_runs() {
    let held = HeldNode::new();
    let (mut server, home, _side) = held.serve(json!([ANSWER])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    chief.say(hand, "Are you up?").await;
    kill_node(&home).await;
    let line = chief.tell(hand, "Are you there?").await;
    let stop = chief.tell(hand, "/stop").await;
    let after = chief.tell(hand, "Hello again.").await;
    let stopped = chief.reply(&stop).await;
    assert_eq!(
        (stopped.kind, stopped.text.as_str()),
        (ReplyKind::Notice, "Stopped 1 running turns.")
    );
    let reply = chief.reply(&line).await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "")
    );
    held.release();
    let reply = chief.reply(&after).await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "")
    );
    let page = chief.open(&chat(hand)).await;
    assert_eq!(shown(&page, "Are you there?"), 1, "{page:?}");
    assert_eq!(
        answers_in(&page),
        [ANSWER, ANSWER],
        "only the lines said before the host ended and after the \
         stop ran: {page:?}"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// The faux model takes 2 s over each answer, so a line the frozen
/// host admitted before the stop reached it would be answered in the
/// chat by the time the line after the thaw is.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_is_answered_within_a_second_with_the_host_frozen() {
    let slow = json!({ "text": ANSWER, "waitMs": 2_000 });
    let (mut server, home, _side) =
        serve(json!(["Up.", slow, slow])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    chief.say(hand, "Are you up?").await;
    let frozen = Frozen::node(&home);
    let line = chief.tell(hand, "Wait for me.").await;
    let sent = Instant::now();
    let stopped = chief
        .request(|id| {
            ClientMessage::Stop(Stop {
                agent: Some(hand),
                id,
            })
        })
        .await;
    let took = sent.elapsed();
    assert_eq!(
        (stopped.kind, stopped.text.as_str()),
        (ReplyKind::Notice, "Stopped 1 running turns.")
    );
    assert!(took < FROZEN_BOUND, "the stop took {took:?}");
    let reply = chief.reply(&line).await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, ""),
        "the line is answered while the host is still frozen"
    );
    frozen.thaw(&server);
    let back = chief.say(hand, "Hello again.").await;
    assert_eq!(back.kind, ReplyKind::Notice, "{}", back.text);
    let page = chief.open(&chat).await;
    assert_eq!(shown(&page, "Wait for me."), 1, "{page:?}");
    assert_eq!(
        answers_in(&page),
        ["Up.", ANSWER],
        "the stopped line has no answer: {page:?}"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// Each line that runs is answered [`ANSWER`], as in
/// [`a_line_a_stop_withdrew_is_answered_and_never_runs`].
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_from_another_connection_withdraws_the_line() {
    let held = HeldNode::new();
    let (mut server, home, _side) = held.serve(json!([ANSWER])).await;
    let mut chief = Client::join(&server).await;
    let mut other = Client::join(&server).await;
    let hand = chief.hand();
    chief.say(hand, "Are you up?").await;
    kill_node(&home).await;
    let line = chief.tell(hand, "Are you there?").await;
    // Answered in its turn, so after the realm has the line.
    let marker = chief.say(AgentId(u64::MAX), "No one.").await;
    assert_eq!(marker.kind, ReplyKind::Error, "{}", marker.text);
    let stopped = other
        .request(|id| {
            ClientMessage::Stop(Stop {
                agent: Some(hand),
                id,
            })
        })
        .await;
    assert_eq!(
        (stopped.kind, stopped.text.as_str()),
        (ReplyKind::Notice, "Stopped 1 running turns.")
    );
    let reply = chief.reply(&line).await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "")
    );
    held.release();
    let back = chief.say(hand, "Hello again.").await;
    assert_eq!(back.kind, ReplyKind::Notice, "{}", back.text);
    let page = chief.open(&chat(hand)).await;
    assert_eq!(shown(&page, "Are you there?"), 1, "{page:?}");
    assert_eq!(answers_in(&page), [ANSWER, ANSWER], "{page:?}");
    other.close().await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// With the Node host down, a founded hamlet's mayor stays hired no
/// further, so a line to it waits in the realm until `/stop all`.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_of_all_withdraws_a_line_to_a_hire_being_hired() {
    let held = HeldNode::new();
    let (mut server, home, _side) = held.serve(json!([ANSWER])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    chief.say(hand, "Are you up?").await;
    kill_node(&home).await;
    chief.command(hand, "/create-hamlet Docs").await;
    let founded = until(&server, "the hamlet", |realm| {
        !realm.hamlets.is_empty()
    })
    .await;
    let mayor = founded.hamlets[0].mayor;
    let line = chief.tell(mayor, "Dig the well.").await;
    let stopped = chief.tell(hand, "/stop all").await;
    let stopped = chief.reply(&stopped).await;
    assert_eq!(
        (stopped.kind, stopped.text.as_str()),
        (ReplyKind::Notice, "Stopped 1 running turns.")
    );
    let reply = chief.reply(&line).await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "")
    );
    held.release();
    said_once_hired(&mut chief, mayor, "Are you hired?").await;
    let page = chief.open(&chat(mayor)).await;
    assert_eq!(shown(&page, "Dig the well."), 1, "{page:?}");
    assert_eq!(
        answers_in(&page),
        [ANSWER],
        "only the line said once it was hired ran: {page:?}"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_said_before_new_stays_in_the_session_it_was_said_in()
{
    let held = HeldNode::new();
    let (mut server, home, _side) =
        held.serve(json!([ANSWER, ANSWER, ANSWER, ANSWER])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    chief.say(hand, "Are you up?").await;
    kill_node(&home).await;
    let before = chief.tell(hand, "Before new.").await;
    let new = chief.tell(hand, "/new").await;
    let after = chief.tell(hand, "After new.").await;
    tokio::time::sleep(DOWN_UNTIL_TRIES_SPREAD).await;
    held.release();
    for line in [&before, &new, &after] {
        chief.reply(line).await;
    }
    let page = chief.open(&chat(hand)).await;
    assert_eq!(shown(&page, "Before new."), 0, "{page:?}");
    assert_eq!(shown(&page, "After new."), 1, "{page:?}");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// How many items of `page` show `text`.
fn shown(page: &Transcript, text: &str) -> usize {
    page.items.iter().filter(|item| item.text == text).count()
}

/// The model's answers in `page`.
fn answers_in(page: &Transcript) -> Vec<&str> {
    page.items
        .iter()
        .filter(|item| item.kind == ItemKind::Assistant)
        .map(|item| item.text.as_str())
        .collect()
}

/// The server's Node host, stopped by SIGSTOP until thawed or
/// dropped.
pub(super) struct Frozen {
    pid: String,
    /// When it was stopped, and its processes then.
    seen: String,
}

impl Frozen {
    pub(super) fn node(home: &Scratch) -> Self {
        let pid = std::fs::read_to_string(home.join("demo-node.pid"))
            .unwrap()
            .trim()
            .to_owned();
        assert!(signal("STOP", &pid), "kill -STOP {pid}");
        let at = chrono::Utc::now().format("%H:%M:%S%.6f");
        let seen =
            format!("stopped at {at} UTC:\n{}", processes(&pid));
        Self { pid, seen }
    }

    /// Lets the host run again. Fails with `server`'s log when the
    /// host has ended meanwhile.
    pub(super) fn thaw(self, server: &Server) {
        let frozen = ManuallyDrop::new(self);
        if !signal("CONT", &frozen.pid) {
            frozen.ended(server);
        }
    }

    /// Ends the frozen host with SIGKILL, as a crash ends it. Fails
    /// with `server`'s log when the host has ended meanwhile.
    pub(super) async fn killed(
        self,
        server: &Server,
        home: &Scratch,
    ) {
        let frozen = ManuallyDrop::new(self);
        if hamlet_os::process::has_ended(frozen.pid.parse().unwrap())
            .unwrap()
        {
            frozen.ended(server);
        }
        kill_node(home).await;
    }

    fn ended(&self, server: &Server) -> ! {
        panic!(
            "the frozen Node host {} has ended. It was \
             {}\nNow:\n{}\nThe server's log: {}",
            self.pid,
            self.seen,
            processes(&self.pid),
            server.log()
        );
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        signal("CONT", &self.pid);
    }
}

/// What `ps` shows of the process `pid`, its parent and this test.
pub(super) fn processes(pid: &str) -> String {
    let parent = ps(&["-o", "ppid=", "-p", pid]);
    let listed =
        [pid, parent.trim(), &std::process::id().to_string()]
            .into_iter()
            .filter(|pid| pid.parse::<u32>().is_ok())
            .collect::<Vec<_>>()
            .join(",");
    ps(&["-o", "pid,ppid,pgid,sess,stat,command", "-p", &listed])
}

fn ps(args: &[&str]) -> String {
    #[expect(
        clippy::disallowed_methods,
        reason = "it lists the server's processes, not an agent's \
                  command"
    )]
    let shown = Command::new("ps").args(args).output().unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&shown.stdout),
        String::from_utf8_lossy(&shown.stderr)
    )
}

/// Whether `kill` sent the signal `name` to `pid`.
fn signal(name: &str, pid: &str) -> bool {
    #[expect(
        clippy::disallowed_methods,
        reason = "it signals the server's Node host, not an agent's \
                  command"
    )]
    let sent = Command::new("kill")
        .args([&format!("-{name}"), pid])
        .status()
        .unwrap();
    sent.success()
}
