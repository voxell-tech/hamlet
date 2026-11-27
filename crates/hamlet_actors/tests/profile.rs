//! The profile through a host: a realm stand-in and an agent
//! stand-in call it as themselves.

#[path = "profile/dreams.rs"]
mod dreams;
#[path = "profile/files.rs"]
mod files;
#[path = "profile/merges.rs"]
mod merges;
#[path = "profile/versions.rs"]
mod versions;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory, ShutdownKind};
use hamlet_actor_error::ActorError;
use hamlet_actors::profile;
use hamlet_git::{Change, MAIN};
use hamlet_host::{Address, Caller, Host, HostOptions, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::profile::{
    Branch, By, Character, CommitId, CommitSeen, Missing, Moved,
    ProfileInput, Removal, RemovePath, Revision, Sha1, Tag,
    TagAnswer, TagRefusal, VersionList, VersionName, VersionSeen,
    Versions,
};
use hamlet_messages::realm::{Done, ProfileId};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;
use hamlet_testing::Scratch;
use serde::{Deserialize, Serialize};

const PEERS: [&str; 2] = [address::Realm::KIND, address::Agent::KIND];

const CHARACTER: &str = "A careful reader.";

/// The messages an agent stand-in was told.
#[derive(Default)]
struct Told(Mutex<Vec<(String, Moved)>>);

/// Stands in for the realm and an agent: calls profile 1 as itself,
/// and keeps what it is told. Asked to remove a path, agent 7 removes
/// it, agent 8 removes it later, and any other fails.
struct Peer {
    told: Arc<Told>,
}

#[derive(Serialize, Deserialize)]
struct Forward {
    name: String,
    args: Vec<u8>,
}

impl Action for Forward {
    type Output = Vec<u8>;
    const NAME: &'static str = "forward";
}

#[async_trait::async_trait]
impl Actor for Peer {
    type Input = ();
    type Actions = (Forward, Moved, RemovePath);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        let told = HostRef::of(ctx.inner())?.value::<Told>();
        Ok(Self {
            told: told.context("the test keeps what is told")?,
        })
    }
}

impl Handler<Forward> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        forward: Forward,
    ) -> anyhow::Result<Vec<u8>> {
        HostRef::of(ctx.inner())?
            .call(
                &profile_at(1),
                &forward.name,
                forward.args,
                Duration::from_secs(20),
            )
            .await
    }
}

impl Handler<Moved> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        moved: Moved,
    ) -> anyhow::Result<()> {
        let mut told = self.told.0.lock().unwrap();
        told.push((ctx.actor_id().to_owned(), moved));
        Ok(())
    }
}

impl Handler<RemovePath> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: RemovePath,
    ) -> anyhow::Result<Removal> {
        let id = ctx.actor_id();
        if id == agent_at(7).to_string() {
            Ok(Removal::Removed)
        } else if id == agent_at(8).to_string() {
            Ok(Removal::Later)
        } else {
            anyhow::bail!("{id} cannot remove it")
        }
    }
}

fn peer(kind: &str) -> ActorFactory {
    let mut registry = Registry::new();
    registry
        .register_actor_with::<Peer>(kind, ActorConfig::default());
    registry.into_factories().remove(kind).unwrap()
}

fn profile_at(id: u32) -> Address {
    address::Profile(ProfileId(id)).address().unwrap()
}

fn realm() -> Address {
    address::Realm.address().unwrap()
}

fn agent() -> Address {
    agent_at(7)
}

fn agent_at(number: u32) -> Address {
    address::Agent(AgentNumber(number)).address().unwrap()
}

fn open_with(dir: &Path, told: Arc<Told>) -> Host {
    let mut kinds = HashMap::from([(
        address::Profile::KIND.to_owned(),
        profile::factory(),
    )]);
    for kind in PEERS {
        kinds.insert(kind.to_owned(), peer(kind));
    }
    Host::open_with(
        dir,
        kinds,
        HostOptions::default().with_value(told),
    )
    .unwrap()
}

fn open(dir: &Path) -> Host {
    open_with(dir, Arc::default())
}

async fn create(
    host: &Host,
    id: u32,
    input: &ProfileInput,
) -> anyhow::Result<()> {
    let mut bytes = Vec::new();
    ciborium::into_writer(input, &mut bytes)?;
    host.create(&profile_at(id), bytes).await
}

fn character() -> ProfileInput {
    ProfileInput::Character {
        text: CHARACTER.into(),
    }
}

async fn made(dir: &Path) -> Host {
    let host = open(dir);
    create(&host, 1, &character()).await.unwrap();
    host
}

async fn call_as<A: Action>(
    host: &Host,
    from: &Address,
    asked: &A,
) -> anyhow::Result<A::Output> {
    let bytes = answer_of(host, from, asked).await?;
    Ok(ciborium::from_reader(bytes.as_slice())?)
}

/// The answer as the host carried it to `from`, not yet decoded.
async fn answer_of<A: Action>(
    host: &Host,
    from: &Address,
    asked: &A,
) -> anyhow::Result<Vec<u8>> {
    host.get_or_start(from).await?;
    let forward = Forward {
        name: A::NAME.to_owned(),
        args: hamlet_actor::action::encode_positional(asked)?,
    };
    let reply = host
        .call(
            from,
            Forward::NAME,
            hamlet_actor::action::encode_positional(&forward)?,
            Duration::from_secs(20),
        )
        .await?;
    ciborium::from_reader(reply.as_slice())
        .context("the peer's reply")
}

fn error_code<T: std::fmt::Debug>(
    reply: anyhow::Result<T>,
) -> String {
    let error = ActorError::extract(&reply.unwrap_err());
    format!("{}.{}", error.group(), error.code())
}

fn tag(id: &RequestId, version: &str, note: &str) -> Tag {
    Tag {
        id: id.clone(),
        version: VersionName(version.into()),
        at: None,
        note: note.into(),
        by: By::Chief { chief: 3 },
    }
}

fn repository(dir: &Path, id: u32) -> hamlet_git::Repository {
    hamlet_git::Repository::open(
        &dir.join(format!("scholars/{id}.git")),
    )
    .unwrap()
}

fn tip(dir: &Path, branch: &str) -> Option<hamlet_git::CommitId> {
    repository(dir, 1).tip(branch).unwrap()
}

fn main_tip(dir: &Path) -> hamlet_git::CommitId {
    tip(dir, MAIN).unwrap()
}

fn commit_id(commit: hamlet_git::CommitId) -> CommitId {
    CommitId(Sha1::from_hex(&commit.to_string()).unwrap())
}

/// A host with profile 1 whose `main` holds `files` beside its
/// character, and hire 7's branch made from it.
async fn hired(
    dir: &Path,
    files: &[Change],
    told: Arc<Told>,
) -> Host {
    let host = open_with(dir, told);
    create(&host, 1, &character()).await.unwrap();
    if !files.is_empty() {
        commit_main(dir, files);
    }
    hire(&host, 7).await;
    host
}

async fn hire(host: &Host, number: u32) {
    let branch = Branch {
        name: format!("hires/{number}"),
        from: None,
    };
    call_as(host, &realm(), &branch).await.unwrap();
}

/// Moves `main` with a commit of the test's own, as an edit does.
fn commit_main(
    dir: &Path,
    changes: &[Change],
) -> hamlet_git::CommitId {
    let by = hamlet_git::Author {
        name: "chief:3",
        at: 5000,
    };
    repository(dir, 1)
        .commit(MAIN, changes, "edited", by)
        .unwrap()
}

fn last_words(dir: &Path, id: u32, branch: &str) -> (String, String) {
    let repository = repository(dir, id);
    let tip = repository.tip(branch).unwrap().unwrap();
    let commit = repository.history(tip, 1).unwrap().remove(0);
    (commit.message, commit.author)
}

fn write(path: &str, bytes: impl Into<Vec<u8>>) -> Change {
    Change::Write {
        path: path.into(),
        bytes: bytes.into(),
    }
}

fn text(dir: &Path, at: CommitId, path: &str) -> Option<String> {
    let at = at.0.to_string().parse().unwrap();
    let bytes = repository(dir, 1).read(at, path).unwrap()?;
    Some(String::from_utf8(bytes).unwrap())
}

/// The first `Moved` agent `number` was told, within 5 s.
async fn moved_to(told: &Told, number: u32) -> Moved {
    let to = agent_at(number).to_string();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let heard = told
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|(at, _)| *at == to)
                .cloned();
            if let Some((_, moved)) = heard {
                return moved;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the agent is told within 5s")
}

/// Waits until agent `number` is told its branch is at `tip`.
async fn told_at(
    told: &Told,
    number: u32,
    tip: hamlet_git::CommitId,
) {
    let (to, tip) = (agent_at(number).to_string(), commit_id(tip));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !told
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|(at, moved)| *at == to && moved.tip == tip)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the agent is told within 5s");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tag_is_kept_and_its_repeat_answered_from_the_marker() {
    let dir = Scratch::new("profile-tag");
    let host = made(&dir).await;
    let id = RequestId::new();
    let first = call_as(&host, &realm(), &tag(&id, "v1", "first"))
        .await
        .unwrap();
    let tip = commit_id(main_tip(&dir));
    assert_eq!(
        first,
        TagAnswer::Tagged {
            version: VersionName("v1".into()),
            commit: tip
        }
    );
    let again = call_as(&host, &realm(), &tag(&id, "v1", "first"))
        .await
        .unwrap();
    assert_eq!(again, first);
    let other =
        call_as(&host, &realm(), &tag(&id, "v1", "another note"))
            .await;
    assert_eq!(error_code(other), "message.id_reused");
    let listed: VersionList =
        call_as(&host, &realm(), &Versions).await.unwrap();
    let at = listed.versions.first().map_or(0, |seen| seen.at);
    assert!(at > 0, "a version is made at a time");
    assert_eq!(
        listed.versions,
        [VersionSeen {
            version: VersionName("v1".into()),
            commit: tip,
            note: "first".into(),
            by: "chief:3".into(),
            at,
        }]
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tag_at_a_commit_git_fails_to_read_fails_and_tags_nothing()
{
    let dir = Scratch::new("profile-tag-damaged");
    let host = made(&dir).await;
    let damaged: hamlet_git::CommitId =
        "cd".repeat(20).parse().unwrap();
    let hex = damaged.to_string();
    let folder = dir.join("scholars/1.git/objects").join(&hex[..2]);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(&hex[2..]), b"not a git object")
        .unwrap();
    let asked = Tag {
        at: Some(Revision::Commit {
            commit: commit_id(damaged),
        }),
        ..tag(&RequestId::new(), "v1", "")
    };
    call_as(&host, &realm(), &asked).await.unwrap_err();
    assert!(repository(&dir, 1).versions().unwrap().is_empty());
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_caller_is_refused_before_anything() {
    let dir = Scratch::new("profile-caller");
    let host = made(&dir).await;
    let asked =
        call_as(&host, &agent(), &tag(&RequestId::new(), "v1", ""))
            .await;
    assert_eq!(error_code(asked), "message.wrong_caller");
    let read = call_as(&host, &agent(), &Versions).await;
    assert_eq!(error_code(read), "message.wrong_caller");
    let branch = Branch {
        name: "hires/7".into(),
        from: None,
    };
    assert_eq!(
        error_code(call_as(&host, &agent(), &branch).await),
        "message.wrong_caller"
    );
    assert_eq!(tip(&dir, "hires/7"), None);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_shape_keeps_nothing_and_frees_its_id() {
    let dir = Scratch::new("profile-shape");
    let host = made(&dir).await;
    let id = RequestId::new();
    let bad = call_as(&host, &realm(), &tag(&id, "../v1", ""))
        .await
        .unwrap();
    assert_eq!(
        bad,
        TagAnswer::Refused {
            why: TagRefusal::BadName {
                version: VersionName("../v1".into())
            }
        }
    );
    let taken_first =
        call_as(&host, &realm(), &tag(&RequestId::new(), "v1", "a"))
            .await
            .unwrap();
    assert!(matches!(taken_first, TagAnswer::Tagged { .. }));
    let taken = call_as(&host, &realm(), &tag(&id, "v1", "b"))
        .await
        .unwrap();
    assert_eq!(
        taken,
        TagAnswer::Refused {
            why: TagRefusal::Taken {
                version: VersionName("v1".into())
            }
        }
    );
    let main = call_as(&host, &realm(), &tag(&id, "main", "b"))
        .await
        .unwrap();
    assert!(matches!(
        main,
        TagAnswer::Refused {
            why: TagRefusal::Taken { .. }
        }
    ));
    let good = call_as(&host, &realm(), &tag(&id, "v2", "b"))
        .await
        .unwrap();
    assert!(matches!(good, TagAnswer::Tagged { .. }));
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_input_outlives_a_restart_and_the_repository_is_not_made_again()
 {
    let dir = Scratch::new("profile-restart");
    let host = made(&dir).await;
    call_as(&host, &realm(), &tag(&RequestId::new(), "v1", "kept"))
        .await
        .unwrap();
    let tip = main_tip(&dir);
    host.shutdown().await.unwrap();
    drop(host);

    let host = open(&dir);
    let listed = call_as(&host, &realm(), &Versions).await.unwrap();
    assert_eq!(listed.versions.len(), 1);
    assert_eq!(main_tip(&dir), tip);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tag_cut_off_before_its_marker_is_found_by_its_repeat() {
    let dir = Scratch::new("profile-cut");
    let host = made(&dir).await;
    host.get_or_start(&profile_at(1)).await.unwrap();
    host.shutdown().await.unwrap();
    drop(host);
    // What a kill after the tag's ref and before the store's commit
    // leaves: the tag, and no marker.
    let tip = main_tip(&dir);
    let tagger = hamlet_git::Author {
        name: "chief:3",
        at: 1,
    };
    repository(&dir, 1).tag("v1", tip, "first", tagger).unwrap();
    // A lock a kill left behind is removed at the start.
    let lock = dir.join("scholars/1.git/refs/heads/main.lock");
    std::fs::write(&lock, b"").unwrap();

    let host = open(&dir);
    let id = RequestId::new();
    let repeat = call_as(&host, &realm(), &tag(&id, "v1", "first"))
        .await
        .unwrap();
    assert_eq!(
        repeat,
        TagAnswer::Tagged {
            version: VersionName("v1".into()),
            commit: commit_id(tip)
        }
    );
    assert!(!lock.exists());
    let again = call_as(&host, &realm(), &tag(&id, "v1", "first"))
        .await
        .unwrap();
    assert_eq!(again, repeat);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn tags_on_one_profile_run_one_at_a_time() {
    let dir = Scratch::new("profile-many");
    let host = Arc::new(made(&dir).await);
    host.get_or_start(&realm()).await.unwrap();
    let mut running = Vec::new();
    for n in 0..16 {
        let host = host.clone();
        running.push(tokio::spawn(async move {
            call_as(
                &host,
                &realm(),
                &tag(&RequestId::new(), "same", &format!("{n}")),
            )
            .await
        }));
    }
    let mut tagged = 0;
    for run in running {
        match run.await.unwrap().unwrap() {
            TagAnswer::Tagged { .. } => tagged += 1,
            TagAnswer::Refused {
                why: TagRefusal::Taken { .. },
            } => {}
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(tagged, 1);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_started_without_its_input_fails_and_makes_nothing()
{
    let dir = Scratch::new("profile-no-input");
    let host = open(&dir);
    let started =
        host.get_or_start(&profile_at(9)).await.unwrap_err();
    let reason =
        format!("{:?}", ActorError::extract(&started).metadata());
    assert!(
        reason.contains("a profile is created with its input"),
        "{reason}"
    );
    assert!(!dir.join("scholars/9.git").exists());
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_made_with_a_character_too_long_fails_and_makes_nothing()
 {
    let dir = Scratch::new("profile-long-character");
    let host = open(&dir);
    let most = ProfileInput::Character {
        text: "a".repeat(4000),
    };
    create(&host, 8, &most).await.unwrap();
    let long = ProfileInput::Character {
        text: "a".repeat(4001),
    };
    let made = create(&host, 9, &long).await.unwrap_err();
    let reason =
        format!("{:?}", ActorError::extract(&made).metadata());
    assert!(
        reason
            .contains("a character may have at most 4000 characters"),
        "{reason}"
    );
    assert!(!dir.join("scholars/9.git").exists());
    assert_eq!(
        last_words(&dir, 8, MAIN),
        ("Made the scholar".to_owned(), "realm".to_owned())
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_versions_list_main_s_latest_commits_by_their_first_line()
{
    let dir = Scratch::new("profile-versions-commits");
    let host = made(&dir).await;
    for n in 1..=13 {
        let by = hamlet_git::Author {
            name: "chief:3",
            at: 5000 + n,
        };
        let change = write(&format!("memory/{n}.md"), "x");
        repository(&dir, 1)
            .commit(MAIN, &[change], &format!("Line {n}\nmore"), by)
            .unwrap();
    }
    let listed: VersionList =
        call_as(&host, &realm(), &Versions).await.unwrap();
    assert_eq!(listed.commits.len(), 12);
    assert_eq!(
        listed.commits[0],
        CommitSeen {
            commit: commit_id(main_tip(&dir)),
            message: "Line 13".into(),
            by: "chief:3".into(),
        }
    );
    assert_eq!(listed.commits[11].message, "Line 2");
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tag_of_a_version_that_exists_is_refused_once_main_moved() {
    let dir = Scratch::new("profile-tag-again");
    let host = made(&dir).await;
    let first = tag(&RequestId::new(), "v1", "");
    let tagged = call_as(&host, &realm(), &first).await.unwrap();
    assert!(matches!(tagged, TagAnswer::Tagged { .. }), "{tagged:?}");
    commit_main(&dir, &[write("memory/a.md", "a")]);

    let again = tag(&RequestId::new(), "v1", "");
    assert_eq!(
        call_as(&host, &realm(), &again).await.unwrap(),
        TagAnswer::Refused {
            why: TagRefusal::Taken {
                version: VersionName("v1".into())
            }
        }
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_same_tag_asked_again_under_another_id_is_refused() {
    let dir = Scratch::new("profile-tag-same");
    let host = made(&dir).await;
    let tagged = call_as(
        &host,
        &realm(),
        &tag(&RequestId::new(), "v1", "first"),
    )
    .await
    .unwrap();
    assert!(matches!(tagged, TagAnswer::Tagged { .. }), "{tagged:?}");

    let again = call_as(
        &host,
        &realm(),
        &tag(&RequestId::new(), "v1", "first"),
    )
    .await
    .unwrap();
    assert_eq!(
        again,
        TagAnswer::Refused {
            why: TagRefusal::Taken {
                version: VersionName("v1".into())
            }
        }
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_keeps_its_repository_under_its_own_number() {
    let dir = Scratch::new("profile-own");
    let host = open(&dir);
    create(&host, 12, &character()).await.unwrap();
    let repository = repository(&dir, 12);
    let main = repository.tip(MAIN).unwrap().unwrap();
    let text =
        repository.read(main, "CHARACTER.md").unwrap().unwrap();
    assert_eq!(text, CHARACTER.as_bytes());
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fork_starts_from_the_files_of_another_profile_s_commit() {
    let dir = Scratch::new("profile-fork");
    let host = made(&dir).await;
    let first = main_tip(&dir);
    host.shutdown().await.unwrap();
    drop(host);
    let later = Change::Write {
        path: "memory/later.md".into(),
        bytes: b"later".to_vec(),
    };
    let by = hamlet_git::Author {
        name: "chief:3",
        at: 5000,
    };
    repository(&dir, 1)
        .commit(MAIN, &[later], "later", by)
        .unwrap();

    let host = open(&dir);
    let fork = ProfileInput::Fork {
        profile: ProfileId(1),
        at: commit_id(first),
    };
    create(&host, 2, &fork).await.unwrap();
    let forked = repository(&dir, 2);
    let main = forked.tip(MAIN).unwrap().unwrap();
    let files = forked.files(main).unwrap();
    assert_eq!(
        files,
        [("CHARACTER.md".to_owned(), CHARACTER.as_bytes().to_vec())]
    );
    assert_eq!(forked.history(main, 10).unwrap().len(), 1);
    assert_eq!(
        last_words(&dir, 2, MAIN),
        ("Forked".to_owned(), "realm".to_owned())
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_branch_is_made_at_main_and_found_by_its_repeat() {
    let dir = Scratch::new("profile-branch");
    let host = made(&dir).await;
    let branch = Branch {
        name: "hires/7".into(),
        from: None,
    };
    call_as(&host, &realm(), &branch).await.unwrap();
    assert_eq!(tip(&dir, "hires/7"), Some(main_tip(&dir)));
    call_as(&host, &realm(), &branch).await.unwrap();
    for name in ["trunk", "hires/07", "hires/"] {
        let bad = Branch {
            name: name.into(),
            from: None,
        };
        call_as(&host, &realm(), &bad).await.unwrap_err();
    }
    let branches = repository(&dir, 1).branches().unwrap();
    let names = branches.into_iter().map(|(name, _)| name);
    assert_eq!(names.collect::<Vec<_>>(), ["hires/7", "main"]);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_is_told_once_main_moved_under_it() {
    let dir = Scratch::new("profile-told");
    let host = made(&dir).await;
    let branch = Branch {
        name: "hires/7".into(),
        from: None,
    };
    call_as(&host, &realm(), &branch).await.unwrap();
    host.shutdown().await.unwrap();
    drop(host);
    let character = Change::Write {
        path: "CHARACTER.md".into(),
        bytes: b"A bolder reader.".to_vec(),
    };
    let by = hamlet_git::Author {
        name: "chief:3",
        at: 5000,
    };
    let main = repository(&dir, 1)
        .commit(MAIN, &[character], "edited", by)
        .unwrap();

    let told = Arc::new(Told::default());
    let host = open_with(&dir, told.clone());
    host.get_or_start(&agent()).await.unwrap();
    call_as(&host, &realm(), &tag(&RequestId::new(), "v1", ""))
        .await
        .unwrap();
    assert_eq!(tip(&dir, "hires/7"), Some(main));
    let moved = Moved {
        tip: commit_id(main),
        character: Some(Character {
            version: 2,
            text: "A bolder reader.".into(),
        }),
    };
    assert_eq!(moved_to(&told, 7).await, moved);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_branch_starts_at_a_branch_a_tag_or_a_commit_and_never_main()
 {
    // As today's hire at a version (`hamlet_server/src/commands.rs`
    // `existing`, `hamlet_git` `resolve`): a branch, then a tag, then
    // a full commit id; `main` itself is no version.
    let dir = Scratch::new("profile-branch-from");
    let host = made(&dir).await;
    hire(&host, 7).await;
    let first = main_tip(&dir);
    let second = commit_main(&dir, &[write("notes.md", "more")]);
    call_as(&host, &realm(), &tag(&RequestId::new(), "v1", ""))
        .await
        .unwrap();
    let seven = tip(&dir, "hires/7");
    let from = |number: u32, name: &str| Branch {
        name: format!("hires/{number}"),
        from: Some(VersionName(name.to_owned())),
    };

    for (number, name) in [
        (8, "hires/7".to_owned()),
        (9, "v1".to_owned()),
        (10, first.to_string()),
    ] {
        let made = call_as(&host, &realm(), &from(number, &name))
            .await
            .unwrap();
        assert_eq!(made, Done::Done, "from {name}");
    }
    let at_main =
        call_as(&host, &realm(), &from(11, MAIN)).await.unwrap();

    assert_eq!(
        [8, 9, 10]
            .map(|number| tip(&dir, &format!("hires/{number}"))),
        [seven, Some(second), Some(first)]
    );
    assert_eq!(
        at_main,
        Done::Refused {
            why: Missing::NoVersion {
                profile: ProfileId(1),
                version: VersionName(MAIN.to_owned()),
            }
        }
    );
    assert_eq!(tip(&dir, "hires/11"), None);
    host.shutdown().await.unwrap();
}
