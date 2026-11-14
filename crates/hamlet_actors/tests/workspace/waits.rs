//! Calls that wait for a chief: the wait, the realm told, the chief's
//! answer, and the check again before a call a chief allowed runs.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use hamlet_actor::Action;
use hamlet_host::{Cause, Host};
use hamlet_messages::agent::Outcome;
use hamlet_messages::realm::NeedsChief;
use hamlet_messages::workspace::files::{ChangeAnswer, Hashed};
use hamlet_messages::workspace::reach::{
    Access, Footing, Mode, Need, Project, Reach, Refusal,
    SecretsVersion,
};
use hamlet_messages::workspace::{
    Answer, AnswerTaken, CallId, CallRefusal, Changed, ChiefAnswer,
    ConversationId, Exit, Lapse, ReachVersion, RealmRefusal,
    RealmTask, RunAnswer, Scope,
};
use hamlet_testing::Scratch;

use super::changes::{changed, edit, manual, read, sha256, write};
use super::files::{ask, project};
use super::{
    AGENT, Leave, Peers, SECRET, agent, agent_runs, files_holding,
    filesystem, read_only, realm, realm_sets, run, told, told_within,
};

/// A host on `dir` whose peers share `peers`, with agent 7's
/// filesystem made, as the agent makes it.
async fn host_with(dir: &Path, peers: Arc<Peers>) -> Host {
    filesystem(dir);
    super::host_with(dir, peers).await
}

/// Answers agent 7's call `call` as the realm, with `answer`.
pub(super) async fn chief(
    host: &Host,
    call: &str,
    answer: Answer,
) -> AnswerTaken {
    ask(
        host,
        &realm(),
        &ChiefAnswer {
            agent: AGENT,
            call: CallId(call.to_owned()),
            answer,
        },
    )
    .await
}

fn asked(peers: &Peers) -> Vec<NeedsChief> {
    peers.asked.lock().unwrap().clone()
}

/// The workspace's rows of waits.
pub(super) fn waits(dir: &Path) -> i64 {
    read_only(dir)
        .query_row("SELECT count(*) FROM waits", [], |row| row.get(0))
        .unwrap()
}

/// How the outcomes the agent was told read.
fn outcomes(peers: &Peers) -> Vec<Outcome> {
    told(peers)
        .into_iter()
        .map(|(_, finished)| finished.outcome)
        .collect()
}

/// Agent 7's reach on `folder`, with the network, in manual mode.
fn manual_with_network(folder: &Path) -> Reach {
    Reach::Agent(Footing {
        project: Some(Project {
            folder: folder.to_owned(),
            mode: Mode::Manual,
        }),
        network: true,
        trust: Vec::new(),
        secrets: SecretsVersion(1),
    })
}

/// In manual mode a command waits for a chief: the call answers so,
/// the realm is told once with the call's task and its words, and
/// nothing runs. A repeat answers that it waits and tells no one; a
/// repeat with another task is another request.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_command_that_needs_a_chief_waits_and_the_realm_is_told() {
    let dir = Scratch::new("workspace-wait-command");
    let (_project, folder) = project("workspace-wait-command-p", &[]);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let mut asked_run = run("c1", "touch marker");
    asked_run.realm_task = Some(RealmTask("task-1".into()));

    let first = agent_runs(&host, &asked_run).await;
    let again = agent_runs(&host, &asked_run).await;
    let mut other_task = asked_run.clone();
    other_task.realm_task = Some(RealmTask("task-2".into()));
    let reused = agent_runs(&host, &other_task).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    host.shutdown().await.unwrap();

    assert_eq!(first, RunAnswer::WaitsForChief(vec![Need::Command]));
    assert_eq!(again, first);
    assert_eq!(
        reused,
        RunAnswer::Refused(CallRefusal::CallReused(CallId(
            "c1".into()
        )))
    );
    assert_eq!(
        asked(&peers),
        [NeedsChief {
            agent: AGENT,
            call: CallId("c1".into()),
            conversation: ConversationId("c1".into()),
            realm_task: Some(RealmTask("task-1".into())),
            needs: vec![Need::Command],
            subject: "touch marker".into(),
            change: None,
            reach_version: ReachVersion(1),
        }]
    );
    assert!(!folder.join("marker").exists());
    assert_eq!(waits(&dir), 1);
    assert_eq!(told(&peers), []);
}

/// A chief allows a waiting command: it runs, the agent is called
/// with its end, and the wait is gone. A second answer finds no wait
/// and changes nothing; a repeat of the call answers its end.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_command_a_chief_allows_runs_and_the_agent_hears_its_end() {
    let dir = Scratch::new("workspace-wait-allow");
    let (_project, folder) = project("workspace-wait-allow-p", &[]);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let asked_run = run("c1", "touch marker; echo made");
    agent_runs(&host, &asked_run).await;

    let taken = chief(&host, "c1", Answer::Allow(Scope::Once)).await;
    told_within(&peers, 1, Duration::from_secs(10)).await;
    let again = chief(&host, "c1", Answer::Allow(Scope::Once)).await;
    let repeat = agent_runs(&host, &asked_run).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    host.shutdown().await.unwrap();

    assert_eq!(taken, AnswerTaken::Runs);
    assert_eq!(again, AnswerTaken::NoWait);
    let outcomes = outcomes(&peers);
    let [Outcome::Ran(end)] = outcomes.as_slice() else {
        panic!("the agent was told {outcomes:?}");
    };
    assert_eq!(
        (&end.exit, end.output.as_str()),
        (&Exit::Code(0), "made\n")
    );
    assert_eq!(repeat, RunAnswer::Done(end.clone()));
    assert!(folder.join("marker").exists());
    assert_eq!(waits(&dir), 0);
}

/// A chief's refusal and a lapse each end their wait: nothing runs,
/// the agent is told why once, and a repeat of the call answers it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_and_a_lapse_end_the_wait_and_run_nothing() {
    let dir = Scratch::new("workspace-wait-refuse");
    let (_project, folder) = project("workspace-wait-refuse-p", &[]);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    agent_runs(&host, &run("c1", "touch one")).await;
    agent_runs(&host, &run("c2", "touch two")).await;

    let refused = chief(
        &host,
        "c1",
        Answer::Refuse {
            reason: Some("not now".into()),
        },
    )
    .await;
    let lapsed =
        chief(&host, "c2", Answer::Lapsed(Lapse::Rewound)).await;
    told_within(&peers, 2, Duration::from_secs(5)).await;
    let repeat = agent_runs(&host, &run("c1", "touch one")).await;
    host.shutdown().await.unwrap();

    let by_chief = CallRefusal::ChiefRefused {
        reason: Some("not now".into()),
    };
    let by_lapse = CallRefusal::Lapsed(Lapse::Rewound);
    assert_eq!(refused, AnswerTaken::NotRun(by_chief.clone()));
    assert_eq!(lapsed, AnswerTaken::NotRun(by_lapse.clone()));
    assert_eq!(
        outcomes(&peers),
        [
            Outcome::NotRun(by_chief.clone()),
            Outcome::NotRun(by_lapse)
        ]
    );
    assert_eq!(repeat, RunAnswer::Refused(by_chief));
    assert!(!folder.join("one").exists());
    assert!(!folder.join("two").exists());
    assert_eq!(waits(&dir), 0);
}

/// A pending operation is refused after a revoke: a change whose
/// project was taken out of the reach, and a command whose network
/// was taken back, each with both versions and why. Neither runs.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_pending_operation_is_refused_after_a_revoke() {
    let dir = Scratch::new("workspace-wait-revoke");
    let (_project, folder) =
        project("workspace-wait-revoke-p", &[("a.md", "a")]);
    let (_elsewhere, elsewhere) =
        project("workspace-wait-revoke-elsewhere", &[]);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual_with_network(&folder)).await;
    let me = agent(AGENT);
    let file = folder.join("a.md");
    ask(&host, &me, &write("w1", &file, "b")).await;
    agent_runs(&host, &run("c1", "curl -s localhost:9 > fetched"))
        .await;
    realm_sets(&host, 3, manual(&elsewhere)).await;

    let change = chief(&host, "w1", Answer::Allow(Scope::Once)).await;
    let command =
        chief(&host, "c1", Answer::Allow(Scope::Once)).await;
    told_within(&peers, 2, Duration::from_secs(5)).await;
    host.shutdown().await.unwrap();

    let moved = CallRefusal::ReachChanged {
        since: ReachVersion(1),
        now: ReachVersion(3),
        why: Changed::Refused(Refusal::OutsideReach {
            path: file.clone(),
            access: Access::Change,
        }),
    };
    let no_network = CallRefusal::ReachChanged {
        since: ReachVersion(1),
        now: ReachVersion(3),
        why: Changed::Needs(vec![Need::Network, Need::Command]),
    };
    assert_eq!(change, AnswerTaken::NotRun(moved.clone()));
    assert_eq!(command, AnswerTaken::NotRun(no_network.clone()));
    assert_eq!(
        outcomes(&peers),
        [Outcome::NotRun(moved), Outcome::NotRun(no_network)]
    );
    assert_eq!(read(&file), "a");
    assert!(!folder.join("fetched").exists());
}

/// In manual mode a write and an edit wait for a chief, and the realm
/// is shown each in the words of the server's tools, the edit with
/// its change. Allowed, each is carried out and the agent is told its
/// answer.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_change_waits_and_is_carried_out_when_allowed() {
    let dir = Scratch::new("workspace-wait-change");
    let (_project, folder) =
        project("workspace-wait-change-p", &[("a.md", "one\ntwo\n")]);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let me = agent(AGENT);
    let file = folder.join("a.md");
    let made = folder.join("new.md");

    let edited =
        ask(&host, &me, &edit("e1", &file, &[("two", "2")])).await;
    let wrote = ask(&host, &me, &write("w1", &made, "hi")).await;
    let unchanged = read(&file);
    let edit_taken =
        chief(&host, "e1", Answer::Allow(Scope::Once)).await;
    let write_taken =
        chief(&host, "w1", Answer::Allow(Scope::Once)).await;
    told_within(&peers, 2, Duration::from_secs(5)).await;
    host.shutdown().await.unwrap();

    let waits_for = ChangeAnswer::WaitsForChief(vec![Need::Edit]);
    assert_eq!((edited, wrote), (waits_for.clone(), waits_for));
    assert_eq!(unchanged, "one\ntwo\n");
    let asked = asked(&peers);
    assert_eq!(
        asked
            .iter()
            .map(|asked| asked.subject.as_str())
            .collect::<Vec<_>>(),
        [
            format!("{}\nReplace:\ntwo\nWith:\n2", file.display()),
            format!("{}\nA new file:\nhi", made.display()),
        ]
    );
    let diff = asked[0].change.as_ref().expect("an edit's change");
    assert!(diff.patch.contains("-two\n+2"), "{}", diff.patch);
    assert_eq!(asked[1].change, None);
    assert_eq!(
        (edit_taken, write_taken),
        (AnswerTaken::Runs, AnswerTaken::Runs)
    );
    let outcomes = outcomes(&peers);
    let [Outcome::Changed(edit), Outcome::Changed(write)] =
        outcomes.as_slice()
    else {
        panic!("the agent was told {outcomes:?}");
    };
    assert_eq!(changed(edit.clone()).hash, sha256(b"one\n2\n"));
    assert!(changed(write.clone()).created);
    assert_eq!(read(&file), "one\n2\n");
    assert_eq!(read(&made), "hi");
}

/// A file written while a change of it waits is left as the other
/// writer left it when a chief allows the change, which is refused
/// with what the file held and what it holds.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_change_to_a_file_changed_meanwhile_is_refused() {
    let dir = Scratch::new("workspace-wait-stale-file");
    let (_project, folder) =
        project("workspace-wait-stale-file-p", &[("a.md", "a")]);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let file = folder.join("a.md");
    ask(&host, &agent(AGENT), &write("w1", &file, "b")).await;
    std::fs::write(&file, "theirs").unwrap();

    let taken = chief(&host, "w1", Answer::Allow(Scope::Once)).await;
    told_within(&peers, 1, Duration::from_secs(5)).await;
    host.shutdown().await.unwrap();

    let refusal = CallRefusal::FileChanged {
        path: file.clone(),
        was: Hashed::Sha256(sha256(b"a")),
        now: Hashed::Sha256(sha256(b"theirs")),
    };
    assert_eq!(taken, AnswerTaken::NotRun(refusal.clone()));
    assert_eq!(outcomes(&peers), [Outcome::NotRun(refusal)]);
    assert_eq!(read(&file), "theirs");
}

/// A call whose text holds one of the realm's secrets does not wait:
/// it is refused, the realm is not told, and nothing of it is kept.
/// What a chief is shown of a file that holds a secret is scrubbed.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn no_secret_is_kept_or_shown_for_a_wait() {
    let dir = Scratch::new("workspace-wait-secret");
    let (_project, folder) = project(
        "workspace-wait-secret-p",
        &[("env.txt", &format!("key={SECRET}\nname=a\n"))],
    );
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let me = agent(AGENT);
    let file = folder.join("env.txt");

    let script =
        agent_runs(&host, &run("c1", &format!("echo {SECRET}")))
            .await;
    let content =
        ask(&host, &me, &write("w1", &file, &format!("{SECRET}!")))
            .await;
    let over = ask(&host, &me, &write("w2", &file, "name=b\n")).await;
    let edited =
        ask(&host, &me, &edit("e1", &file, &[("name=a", "name=b")]))
            .await;
    host.shutdown().await.unwrap();

    assert_eq!(script, RunAnswer::Refused(CallRefusal::HoldsSecret));
    assert_eq!(
        content,
        ChangeAnswer::Refused(CallRefusal::HoldsSecret)
    );
    let waits_for = ChangeAnswer::WaitsForChief(vec![Need::Edit]);
    assert_eq!((over, edited), (waits_for.clone(), waits_for));
    let asked = asked(&peers);
    assert_eq!(asked.len(), 2);
    for asked in &asked {
        let shown = format!("{asked:?}");
        assert!(!shown.contains(SECRET), "{shown}");
    }
    assert!(asked[0].subject.contains("key=[REDACTED]"));
    assert!(
        asked[1]
            .change
            .as_ref()
            .is_some_and(|change| change.diff.contains("[REDACTED]"))
    );
    assert_eq!(
        files_holding(&dir.join("actors/workspace"), SECRET),
        Vec::<std::path::PathBuf>::new()
    );
    assert_eq!(waits(&dir), 2);
}

/// Only the realm answers a wait: an agent's answer is refused and
/// the wait stays.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn only_the_realm_answers_a_wait() {
    let dir = Scratch::new("workspace-wait-realm-only");
    let (_project, folder) =
        project("workspace-wait-realm-only-p", &[]);
    let host = host_with(&dir, Arc::default()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    agent_runs(&host, &run("c1", "touch marker")).await;

    let taken: AnswerTaken = ask(
        &host,
        &agent(AGENT),
        &ChiefAnswer {
            agent: AGENT,
            call: CallId("c1".into()),
            answer: Answer::Allow(Scope::Always),
        },
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        taken,
        AnswerTaken::Refused(RealmRefusal::NotTheRealm)
    );
    assert_eq!(waits(&dir), 1);
    assert!(!folder.join("marker").exists());
}

/// A wait stays through a restart, and nothing of it ran. Allowed
/// after it, the command runs once the secrets are fetched again.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_wait_stays_through_a_restart() {
    let dir = Scratch::new("workspace-wait-restart");
    let (_project, folder) = project("workspace-wait-restart-p", &[]);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let asked_run = run("c1", "touch marker");
    agent_runs(&host, &asked_run).await;
    host.shutdown().await.unwrap();

    let host = host_with(&dir, peers.clone()).await;
    let repeat = agent_runs(&host, &asked_run).await;
    let fetched_before =
        peers.fetched.load(std::sync::atomic::Ordering::SeqCst);
    let taken = chief(&host, "c1", Answer::Allow(Scope::Once)).await;
    told_within(&peers, 1, Duration::from_secs(10)).await;
    host.shutdown().await.unwrap();

    assert_eq!(repeat, RunAnswer::WaitsForChief(vec![Need::Command]));
    assert_eq!(taken, AnswerTaken::Runs);
    assert_eq!(
        peers.fetched.load(std::sync::atomic::Ordering::SeqCst),
        fetched_before + 1
    );
    assert!(matches!(
        outcomes(&peers).as_slice(),
        [Outcome::Ran(end)] if end.exit == Exit::Code(0)
    ));
    assert!(folder.join("marker").exists());
}

/// When the realm cannot be told that a call waits, nothing tells it
/// again: the call still answers that it waits, the wait stays, and a
/// repeat of the call says it waits.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_wait_the_realm_was_not_told_of_stays() {
    let dir = Scratch::new("workspace-wait-untold");
    let (_project, folder) = project("workspace-wait-untold-p", &[]);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    agent_runs(&host, &run("c1", "touch one")).await;
    host.call(
        &realm(),
        Leave::NAME,
        hamlet_actor::action::encode_positional(&Leave).unwrap(),
        Duration::from_secs(10),
        Cause::root("test"),
    )
    .await
    .unwrap();

    let first = agent_runs(&host, &run("c2", "touch two")).await;
    let repeat = agent_runs(&host, &run("c2", "touch two")).await;
    host.shutdown().await.unwrap();

    let waits_for = RunAnswer::WaitsForChief(vec![Need::Command]);
    assert_eq!((first, repeat), (waits_for.clone(), waits_for));
    assert_eq!(
        asked(&peers)
            .iter()
            .map(|asked| asked.call.0.as_str())
            .collect::<Vec<_>>(),
        ["c1"]
    );
    assert_eq!(waits(&dir), 2);
}

/// A granted command runs under the reach in force: with the network
/// when the reach gives it since, though the chief was asked for it
/// too, and when trust given while it waited now lets it run with no
/// chief. Each runs under the policy of a command that needs none.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_granted_command_runs_under_the_reach_in_force() {
    let dir = Scratch::new("workspace-wait-network");
    let (_project, folder) = project("workspace-wait-network-p", &[]);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let fetches =
        agent_runs(&host, &run("c1", "curl -s localhost:9; true"))
            .await;
    realm_sets(&host, 2, manual_with_network(&folder)).await;
    agent_runs(&host, &run("c2", "true")).await;
    chief(&host, "c1", Answer::Allow(Scope::Once)).await;
    told_within(&peers, 1, Duration::from_secs(10)).await;
    let Reach::Agent(mut trusting) = manual_with_network(&folder)
    else {
        unreachable!()
    };
    trusting.trust = vec![hamlet_messages::workspace::reach::Trust {
        conversation: ConversationId("c1".into()),
        edits: false,
        commands: true,
    }];
    realm_sets(&host, 3, Reach::Agent(trusting)).await;
    let plain = agent_runs(&host, &run("c3", "true")).await;
    chief(&host, "c2", Answer::Allow(Scope::Once)).await;
    told_within(&peers, 2, Duration::from_secs(10)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        fetches,
        RunAnswer::WaitsForChief(vec![Need::Network, Need::Command])
    );
    let RunAnswer::Done(plain) = plain else {
        panic!("a trusted command answered {plain:?}");
    };
    let policies = outcomes(&peers)
        .into_iter()
        .map(|outcome| match outcome {
            Outcome::Ran(end) => (end.exit, end.policy),
            other => panic!("the agent was told {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        policies,
        [
            (Exit::Code(0), plain.policy.clone()),
            (Exit::Code(0), plain.policy)
        ]
    );
}

/// A chief is shown each text of a change cut to 2000 characters, so
/// a write over a long file still shows what it writes.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_chief_is_shown_each_text_of_a_change_cut() {
    let dir = Scratch::new("workspace-wait-long");
    let old = "o".repeat(3000);
    let (_project, folder) =
        project("workspace-wait-long-p", &[("a.md", &old)]);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let file = folder.join("a.md");
    ask(&host, &agent(AGENT), &write("w1", &file, "new")).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        asked(&peers)[0].subject,
        format!(
            "{}\nReplace:\n{}\n[1000 more characters]\nWith:\nnew",
            file.display(),
            &old[..2000]
        )
    );
}
