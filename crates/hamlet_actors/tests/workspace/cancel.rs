//! Ending an agent's commands: a cancel from the realm or the agent,
//! and an agent whose reach ended.

use std::time::{Duration, Instant};

use hamlet_actor::Action;
use hamlet_host::{Address, Host};
use hamlet_messages::agent::Outcome;
use hamlet_messages::workspace::reach::{Reach, Refusal};
use hamlet_messages::workspace::{
    AgentNumber, Answer, AnswerTaken, CallId, CallRefusal,
    CallerRefusal, Cancel, CancelAnswer, ConversationId, Exit, Lapse,
    ReachAnswer, ReachVersion, Run, RunAnswer, SetReach, Which,
};
use hamlet_testing::Scratch;

use super::changes::manual;
use super::files::{ask, project};
use super::waits::{chief, waits};
use super::{
    AGENT, Leave, agent, agent_runs, answering_within, filesystem,
    host_with, ran, reach, realm, realm_sets, run, running, runs_as,
    status_as, told, told_within,
};

/// Asks the workspace, as the actor at `from`, to end agent 7's
/// commands of `which`.
async fn cancels(
    host: &Host,
    from: &Address,
    which: Which,
) -> CancelAnswer {
    ask(
        host,
        from,
        &Cancel {
            agent: AGENT,
            which,
        },
    )
    .await
}

fn one(call: &str) -> Which {
    Which::One(CallId(call.to_owned()))
}

/// A run of `script` as call `call` in the conversation
/// `conversation`.
fn run_in(conversation: &str, call: &str, script: &str) -> Run {
    Run {
        conversation: ConversationId(conversation.to_owned()),
        ..run(call, script)
    }
}

/// A cancel of a command that answered `Running` ends it within a
/// second: its process is gone, the cancel answers its id, and the
/// agent is called once with its end as cancelled, which its status
/// answers too.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_ends_a_running_command_and_the_agent_is_told_once()
{
    let dir = Scratch::new("workspace-cancel-running");
    let fs = filesystem(&dir);
    let peers = answering_within(300);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let command = running(
        &agent_runs(
            &host,
            &run("c1", "echo started; sleep 2; touch late"),
        )
        .await,
    );

    let asked = Instant::now();
    let answer = cancels(&host, &agent(AGENT), one("c1")).await;
    let took = asked.elapsed();
    told_within(&peers, 1, Duration::from_secs(3)).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let status = status_as(&host, AGENT, command).await;
    host.shutdown().await.unwrap();

    assert_eq!(answer, CancelAnswer::Cancelled(vec![command]));
    assert!(took < Duration::from_secs(1), "{took:?}");
    let told = told(&peers);
    assert_eq!(told.len(), 1, "{told:?}");
    let end = ran(&told[0].1);
    assert_eq!(told[0].1.call, CallId("c1".into()));
    assert_eq!(
        (&end.exit, end.output.as_str()),
        (&Exit::Cancelled, "started\n")
    );
    assert!(!fs.join("late").exists(), "the command ran on");
    assert_eq!(status, RunAnswer::Done(end.clone()));
}

/// A cancel while the call still waits for its command ends it, and
/// the call answers `Done` as cancelled: the agent hears the end
/// once, so it is not called.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_within_the_wait_answers_the_call() {
    let dir = Scratch::new("workspace-cancel-waiting");
    let fs = filesystem(&dir);
    let peers = answering_within(5000);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let call = {
        let host = host.clone();
        tokio::spawn(async move {
            agent_runs(&host, &run("c1", "sleep 2; touch late")).await
        })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;

    let asked = Instant::now();
    let answer = cancels(&host, &realm(), Which::All).await;
    let answered = call.await.unwrap();
    let took = asked.elapsed();
    tokio::time::sleep(Duration::from_millis(2000)).await;
    host.shutdown().await.unwrap();

    let RunAnswer::Done(end) = answered else {
        panic!("the call answered {answered:?}");
    };
    assert_eq!(end.exit, Exit::Cancelled);
    assert_eq!(answer, CancelAnswer::Cancelled(vec![end.command]));
    assert!(took < Duration::from_secs(1), "{took:?}");
    assert_eq!(told(&peers), []);
    assert!(!fs.join("late").exists(), "the command ran on");
}

/// A cancel of a conversation ends that conversation's commands
/// only; a cancel of all ends the rest.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_ends_a_conversations_commands_or_all() {
    let dir = Scratch::new("workspace-cancel-conversation");
    filesystem(&dir);
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let mut commands = Vec::new();
    for (conversation, call) in
        [("a", "c1"), ("a", "c2"), ("b", "c3")]
    {
        commands.push(running(
            &agent_runs(
                &host,
                &run_in(conversation, call, "sleep 5"),
            )
            .await,
        ));
    }

    let of_a = cancels(
        &host,
        &realm(),
        Which::Conversation(ConversationId("a".into())),
    )
    .await;
    let b_meanwhile = status_as(&host, AGENT, commands[2]).await;
    let all = cancels(&host, &agent(AGENT), Which::All).await;
    told_within(&peers, 3, Duration::from_secs(3)).await;
    host.shutdown().await.unwrap();

    let mut of_a = match of_a {
        CancelAnswer::Cancelled(of_a) => of_a,
        other => panic!("the cancel answered {other:?}"),
    };
    of_a.sort_by_key(|command| command.0);
    assert_eq!(of_a, commands[..2]);
    assert_eq!(
        b_meanwhile,
        RunAnswer::Running {
            command: commands[2]
        }
    );
    assert_eq!(all, CancelAnswer::Cancelled(vec![commands[2]]));
    let told = told(&peers);
    assert_eq!(told.len(), 3, "{told:?}");
    assert!(
        told.iter().all(
            |(_, finished)| ran(finished).exit == Exit::Cancelled
        ),
        "{told:?}"
    );
}

/// A cancel that finds no running command answers none and changes
/// nothing: an unknown call, a command that ended, or no command at
/// all.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_of_nothing_does_nothing() {
    let dir = Scratch::new("workspace-cancel-nothing");
    filesystem(&dir);
    let peers = answering_within(2000);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let before = cancels(&host, &realm(), Which::All).await;
    let unknown = cancels(&host, &agent(AGENT), one("nope")).await;
    let RunAnswer::Done(end) =
        agent_runs(&host, &run("c1", "echo hi")).await
    else {
        panic!("echo did not answer Done");
    };
    let ended = cancels(&host, &agent(AGENT), one("c1")).await;
    let status = status_as(&host, AGENT, end.command).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    host.shutdown().await.unwrap();

    let none = CancelAnswer::Cancelled(Vec::new());
    assert_eq!(
        [before, unknown, ended],
        [none.clone(), none.clone(), none]
    );
    assert_eq!(end.exit, Exit::Code(0));
    assert_eq!(status, RunAnswer::Done(end));
    assert_eq!(told(&peers), []);
}

/// The commands of a host that stopped died with it, so a cancel
/// after the restart finds nothing, and the command reads cut off.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_after_a_restart_finds_nothing() {
    let dir = Scratch::new("workspace-cancel-restart");
    filesystem(&dir);
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let command =
        running(&agent_runs(&host, &run("c1", "sleep 30")).await);
    host.shutdown().await.unwrap();
    let again = host_with(&dir, peers.clone()).await;
    let answer = cancels(&again, &realm(), Which::All).await;
    let status = status_as(&again, AGENT, command).await;
    again.shutdown().await.unwrap();

    assert_eq!(answer, CancelAnswer::Cancelled(Vec::new()));
    assert!(
        matches!(&status, RunAnswer::Done(end) if end.exit == Exit::CutOff),
        "{status:?}"
    );
    assert_eq!(told(&peers), []);
}

/// Only the realm and the agent named cancel its commands: another
/// agent and another kind are refused, and the command runs on.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_comes_from_the_realm_or_the_agent_it_names() {
    let dir = Scratch::new("workspace-cancel-caller");
    filesystem(&dir);
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let command =
        running(&agent_runs(&host, &run("c1", "sleep 5")).await);
    let other = AgentNumber(8);
    let by_other = cancels(&host, &agent(other), Which::All).await;
    let by_a_stranger = cancels(
        &host,
        &Address::new("other", "x").unwrap(),
        Which::All,
    )
    .await;
    let meanwhile = status_as(&host, AGENT, command).await;
    let by_the_realm = cancels(&host, &realm(), Which::All).await;
    told_within(&peers, 1, Duration::from_secs(3)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        by_other,
        CancelAnswer::Refused(CallerRefusal::NotTheCaller {
            named: AGENT,
            caller: other,
        })
    );
    assert_eq!(
        by_a_stranger,
        CancelAnswer::Refused(CallerRefusal::NotAnAgent)
    );
    assert_eq!(meanwhile, RunAnswer::Running { command });
    assert_eq!(by_the_realm, CancelAnswer::Cancelled(vec![command]));
}

/// Calls the workspace with a [`SetReach`] of agent `number`, as
/// the realm.
pub(super) async fn realm_sets_for(
    host: &Host,
    number: AgentNumber,
    version: u64,
    reach: Reach,
) -> ReachAnswer {
    ask(
        host,
        &realm(),
        &SetReach {
            agent: number,
            version: ReachVersion(version),
            reach,
        },
    )
    .await
}

/// when an agent's reach ends, its running commands end in the
/// handling of that call, so none runs on once the realm has its
/// answer, and the agent is told each once. Another agent's command
/// runs on, and the ended agent starts no command.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_reach_ends_the_agents_commands() {
    let dir = Scratch::new("workspace-ended-commands");
    let fs = filesystem(&dir);
    let other = AgentNumber(8);
    let other_fs = dir.join("actors/agent/8.fs");
    std::fs::create_dir_all(&other_fs).unwrap();
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    realm_sets_for(&host, other, 1, reach(None)).await;
    let command = running(
        &agent_runs(&host, &run("c1", "sleep 1.5; touch late")).await,
    );
    let others = Run {
        agent: other,
        ..run("c1", "sleep 1.5; touch late")
    };
    running(&runs_as(&host, other, &others).await);

    let asked = Instant::now();
    let ended = realm_sets(&host, 2, Reach::Ended).await;
    let took = asked.elapsed();
    let after = agent_runs(&host, &run("c2", "touch after")).await;
    told_within(&peers, 2, Duration::from_secs(3)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let seven = host.get_or_start(super::workspace::AGENT, "7").await;
    host.shutdown().await.unwrap();

    assert_eq!(ended, ReachAnswer::InForce(ReachVersion(2)));
    assert!(took < Duration::from_secs(1), "{took:?}");
    assert!(
        !fs.join("late").exists(),
        "the ended agent's command ran on"
    );
    assert!(
        other_fs.join("late").exists(),
        "the other agent's ended"
    );
    assert_eq!(
        after,
        RunAnswer::Refused(CallRefusal::Reach(Refusal::NoReach))
    );
    assert!(!fs.join("after").exists());
    let told = told(&peers);
    let to_seven = told
        .iter()
        .filter(|(to, _)| *to == seven.as_ref().unwrap().as_str())
        .map(|(_, finished)| (finished.call.clone(), ran(finished)))
        .collect::<Vec<_>>();
    assert_eq!(to_seven.len(), 1, "{told:?}");
    assert_eq!(to_seven[0].0, CallId("c1".into()));
    assert_eq!(
        (to_seven[0].1.command, &to_seven[0].1.exit),
        (command, &Exit::Cancelled)
    );
}

/// an ended reach closes the agent's waits for a chief in the
/// same change: the agent is told its call's wait lapsed, a chief's
/// allow a moment later finds no wait and starts nothing, and a
/// repeat of the call answers that it lapsed.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_reach_closes_the_agents_waits() {
    let dir = Scratch::new("workspace-ended-waits");
    filesystem(&dir);
    let (_project, folder) = project("workspace-ended-waits-p", &[]);
    let peers = answering_within(2000);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let call = run("c1", "touch marker");
    let waiting = agent_runs(&host, &call).await;

    let ended = realm_sets(&host, 2, Reach::Ended).await;
    let allowed = chief(&host, "c1", Answer::Allow).await;
    let repeat = agent_runs(&host, &call).await;
    told_within(&peers, 1, Duration::from_secs(3)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    host.shutdown().await.unwrap();

    assert!(
        matches!(waiting, RunAnswer::WaitsForChief(_)),
        "{waiting:?}"
    );
    assert_eq!(ended, ReachAnswer::InForce(ReachVersion(2)));
    assert_eq!(allowed, AnswerTaken::NoWait);
    assert_eq!(waits(&dir), 0);
    let lapsed = CallRefusal::Lapsed(Lapse::Ended);
    assert_eq!(repeat, RunAnswer::Refused(lapsed.clone()));
    let told = told(&peers)
        .into_iter()
        .map(|(_, finished)| (finished.call, finished.outcome))
        .collect::<Vec<_>>();
    assert_eq!(
        told,
        [(CallId("c1".into()), Outcome::NotRun(lapsed))]
    );
    assert!(!folder.join("marker").exists());
}

/// A chief's allow and the agent's end, called at once, never let a
/// command of the ended agent run: whichever comes first, the agent
/// is told once, and the command's file is never made. Every other
/// round calls with the end a moment later, so both orders occur.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_allow_racing_an_ended_reach_runs_nothing() {
    let dir = Scratch::new("workspace-ended-race");
    filesystem(&dir);
    let (_project, folder) = project("workspace-ended-race-p", &[]);
    let peers = answering_within(2000);
    let host = host_with(&dir, peers.clone()).await;
    let rounds = 20;
    for round in 0..rounds {
        let version = 2 * round + 1;
        realm_sets(&host, version, manual(&folder)).await;
        let call = format!("c{round}");
        let script = format!("sleep 0.5; touch marker{round}");
        let waiting = agent_runs(&host, &run(&call, &script)).await;
        assert!(
            matches!(waiting, RunAnswer::WaitsForChief(_)),
            "{waiting:?}"
        );
        let allow = {
            let (host, call) = (host.clone(), call.clone());
            tokio::spawn(async move {
                chief(&host, &call, Answer::Allow).await
            })
        };
        // A moment later in every other round, so the allow comes
        // first there.
        if round % 2 == 1 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let ended =
            realm_sets(&host, version + 1, Reach::Ended).await;
        let allowed = allow.await.unwrap();
        assert_eq!(
            ended,
            ReachAnswer::InForce(ReachVersion(version + 1))
        );
        assert!(
            matches!(
                allowed,
                AnswerTaken::Runs | AnswerTaken::NoWait
            ),
            "round {round}: {allowed:?}"
        );
    }
    told_within(&peers, rounds as usize, Duration::from_secs(3))
        .await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    host.shutdown().await.unwrap();

    let made = (0..rounds)
        .filter(|round| {
            folder.join(format!("marker{round}")).exists()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        made,
        Vec::<u64>::new(),
        "the ended agent's commands ran"
    );
    let told = told(&peers);
    assert_eq!(told.len(), rounds as usize, "{told:?}");
    for (_, finished) in told {
        assert!(
            matches!(
                &finished.outcome,
                Outcome::Ran(end) if end.exit == Exit::Cancelled
            ) || finished.outcome
                == Outcome::NotRun(CallRefusal::Lapsed(Lapse::Ended)),
            "{finished:?}"
        );
    }
}

/// A call does not make its target: when the ended agent is gone, the
/// notice of its closed wait fails and is dropped, and the realm
/// still has its answer.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_notice_to_an_agent_that_is_gone_is_dropped() {
    let dir = Scratch::new("workspace-ended-gone");
    filesystem(&dir);
    let (_project, folder) = project("workspace-ended-gone-p", &[]);
    let peers = answering_within(2000);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    agent_runs(&host, &run("c1", "touch marker")).await;
    host.call(
        &agent(AGENT),
        Leave::NAME,
        hamlet_actor::action::encode_positional(&Leave).unwrap(),
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    let ended = realm_sets(&host, 2, Reach::Ended).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let agent_file = dir.join("actors/agent/7.sqlite").exists();
    host.shutdown().await.unwrap();

    assert_eq!(ended, ReachAnswer::InForce(ReachVersion(2)));
    assert_eq!(waits(&dir), 0);
    assert_eq!(told(&peers), []);
    assert!(!agent_file, "the notice made the agent again");
}
