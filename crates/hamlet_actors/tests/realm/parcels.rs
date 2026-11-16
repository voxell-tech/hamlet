//! Parcels: the files a letter sends, measured and copied from the
//! sender's workspace into the realm, then fetched by the receiver.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hamlet_actor::{Actor, Ctx, Handler};
use hamlet_actors::realm;
use hamlet_host::{Caller, Host, HostOptions, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::{Input, Submit};
use hamlet_messages::bytes::{Base64, Sha256};
use hamlet_messages::realm::client::AnswerRefusal;
use hamlet_messages::realm::oversight::{Lapse, LapseReason};
use hamlet_messages::realm::parcels::{
    FetchPart, Measure, PARCEL_PART, ParcelFile, ParcelId, Part,
    PartRefusal, part_start,
};
use hamlet_messages::realm::world::WaitKind;
use hamlet_messages::realm::{Done, Enclosed, Tell};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::refusal::{PathRefusal, ToRead};
use hamlet_messages::workspace::{
    AgentNumber, AnswerTaken, ChiefAnswer, InForce, Parcel,
    ParcelAnswer, ParcelEntry, ParcelFault, ParcelPart,
    ParcelRefusal, RealmTask, SetReach,
};
use hamlet_protocol::client::Choice;
use hamlet_testing::Scratch;
use sha2::Digest as _;
use tokio::sync::watch;

use super::asking::{chief_task, held_rows, objection, on};
use super::{
    AgentStandIn, Forward, MAYOR, ProfileStandIn, Realm, Seen, THREE,
    answer, forward, founded, hires, kind, seen, submits_to, tell,
    to, tool, until,
};

/// The files the stand-in workspace holds, by path, and the parts it
/// holds back while a test asks.
#[derive(Default)]
struct Files {
    held: Mutex<BTreeMap<PathBuf, Vec<u8>>>,
    /// Parts from this one on wait until it is cleared.
    hold_from: watch::Sender<Option<u32>>,
    waiting: Mutex<bool>,
}

impl Files {
    fn with(files: &[(&str, &[u8])]) -> Self {
        let made = Self::default();
        made.add(files);
        made
    }

    fn add(&self, files: &[(&str, &[u8])]) {
        let mut held = self.held.lock().unwrap();
        for (path, bytes) in files {
            held.insert(PathBuf::from(path), bytes.to_vec());
        }
    }

    /// What `paths` name, as the real workspace measures them: a
    /// path outside `/project` is out of reach.
    fn measure(&self, paths: &[Enclosed]) -> ParcelAnswer {
        let held = self.held.lock().unwrap();
        let mut files = Vec::new();
        for Enclosed { path: named } in paths {
            if !named.starts_with("/project") {
                let outside = PathRefusal::OutsideReach {
                    path: named.clone(),
                    access: ToRead::Read,
                };
                return ParcelAnswer::Refused {
                    why: ParcelFault::Unreadable {
                        path: named.clone(),
                        words: outside.to_string(),
                    }
                    .into(),
                };
            }
            let from = named.parent().unwrap();
            for (path, bytes) in held.range(named.clone()..) {
                if !path.starts_with(named) {
                    break;
                }
                files.push(ParcelEntry {
                    path: path.clone(),
                    name: path.strip_prefix(from).unwrap().to_owned(),
                    size: bytes.len() as u64,
                    sha256: hash(bytes),
                });
            }
        }
        if files.is_empty() {
            return ParcelAnswer::Refused {
                why: ParcelFault::NoFiles.into(),
            };
        }
        ParcelAnswer::Measured { files }
    }

    /// Part `part` of the file at `path`, as the real workspace
    /// reads it: a part past the file's end is refused.
    fn part(&self, path: &Path, part: u32) -> Part<ParcelRefusal> {
        let held = self.held.lock().unwrap();
        let bytes = &held[path];
        let Some(start) = part_start(bytes.len() as u64, part) else {
            return Part::Refused {
                why: ParcelFault::NoSuchPart {
                    path: path.to_owned(),
                    part,
                }
                .into(),
            };
        };
        let start = start as usize;
        let end = (start + PARCEL_PART).min(bytes.len());
        Part::Bytes {
            bytes: Base64(bytes[start..end].to_vec()),
            last: end == bytes.len(),
        }
    }

    fn hold_from(&self, part: u32) {
        self.hold_from.send_replace(Some(part));
    }

    fn release(&self) {
        self.hold_from.send_replace(None);
    }

    async fn called(&self, part: u32) {
        let mut held = self.hold_from.subscribe();
        let holds = |from: &Option<u32>| {
            from.is_some_and(|from| part >= from)
        };
        if holds(&held.borrow()) {
            *self.waiting.lock().unwrap() = true;
        }
        let _ = held.wait_for(|from| !holds(from)).await;
    }
}

fn hash(bytes: &[u8]) -> Sha256 {
    Sha256(sha2::Sha256::digest(bytes).into())
}

/// A workspace that takes a hire's reach and answers parcels from
/// [`Files`].
struct FilesStandIn {
    seen: Arc<Seen>,
    files: Arc<Files>,
}

#[async_trait::async_trait]
impl Actor for FilesStandIn {
    type Input = ();
    type Actions =
        (SetReach, ChiefAnswer, Parcel, ParcelPart, Forward);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            seen: seen(ctx)?,
            files: HostRef::of(ctx.inner())?
                .value::<Files>()
                .expect("the host holds the workspace's files"),
        })
    }
}

impl Handler<SetReach> for FilesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        set: SetReach,
    ) -> anyhow::Result<InForce> {
        let version = set.version;
        self.seen.reaches.lock().unwrap().push(set);
        Ok(InForce { version })
    }
}

impl Handler<ChiefAnswer> for FilesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        answer: ChiefAnswer,
    ) -> anyhow::Result<AnswerTaken> {
        self.seen.chief_answers.lock().unwrap().push(answer);
        Ok(AnswerTaken::Taken)
    }
}

impl Handler<Parcel> for FilesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        parcel: Parcel,
    ) -> anyhow::Result<ParcelAnswer> {
        assert_eq!(
            parcel.agent, MAYOR,
            "the sender's files are read"
        );
        Ok(self.files.measure(&parcel.paths))
    }
}

impl Handler<ParcelPart> for FilesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        asked: ParcelPart,
    ) -> anyhow::Result<Part<ParcelRefusal>> {
        self.files.called(asked.part).await;
        Ok(self.files.part(&asked.path, asked.part))
    }
}

impl Handler<Forward> for FilesStandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Forward,
    ) -> anyhow::Result<Vec<u8>> {
        forward(&ctx, asked).await
    }
}

async fn open_with(
    dir: &Path,
    seen: &Arc<Seen>,
    files: &Arc<Files>,
) -> Host {
    let kinds = HashMap::from([
        (address::Realm::KIND.to_owned(), realm::factory()),
        (
            address::Agent::KIND.to_owned(),
            kind::<AgentStandIn>(address::Agent::KIND),
        ),
        (
            address::Profile::KIND.to_owned(),
            kind::<ProfileStandIn>(address::Profile::KIND),
        ),
        (
            address::Workspace::KIND.to_owned(),
            kind::<FilesStandIn>(address::Workspace::KIND),
        ),
    ]);
    let options = HostOptions::default()
        .with_value(seen.clone())
        .with_value(files.clone());
    let host = Host::open_with(dir, kinds, options).unwrap();
    for at in [address::Workspace.address(), address::Realm.address()]
    {
        host.get_or_start(&at.unwrap()).await.unwrap();
    }
    host
}

/// A realm whose workspace holds `files`, with mayor 2 and its hire
/// 3 live.
async fn sending(files: Files) -> (Realm, Arc<Files>) {
    let scratch = Scratch::new("realm-parcels");
    let seen = Arc::new(Seen::default());
    let files = Arc::new(files);
    let host = open_with(&scratch, &seen, &files).await;
    let realm = Realm {
        host,
        seen,
        options: None,
        scratch,
    };
    realm.live(Hand::NUMBER).await;
    founded(&realm).await;
    realm.live(MAYOR).await;
    hires(&realm, MAYOR, "h0").await;
    realm.live(THREE).await;
    (realm, files)
}

async fn restart(realm: Realm, files: &Arc<Files>) -> Realm {
    realm.host.shutdown().await.unwrap();
    let host = open_with(&realm.scratch, &realm.seen, files).await;
    Realm { host, ..realm }
}

/// Stops the realm's host while the workspace holds a part, then
/// lets the part go and opens the host again.
async fn killed_while_held(
    realm: Realm,
    files: &Arc<Files>,
) -> Realm {
    let host = realm.host.clone();
    let down = tokio::spawn(async move { host.shutdown().await });
    let at = address::Realm.address().unwrap();
    until("the realm stops", || !realm.host.is_running(&at)).await;
    files.release();
    down.await.unwrap().unwrap();
    let host = open_with(&realm.scratch, &realm.seen, files).await;
    Realm { host, ..realm }
}

/// Mayor 2 tells 3 `text` with the files under `paths`.
async fn sends(
    realm: &Realm,
    call: &str,
    text: &str,
    paths: &[&str],
) {
    let mut sent = tell(to(THREE), text);
    sent.paths = paths
        .iter()
        .map(|path| Enclosed { path: path.into() })
        .collect();
    let answer = realm.tool(MAYOR, &tool(call, sent)).await.unwrap();
    assert_eq!(answer, Done::Done, "tell answers at once");
}

/// The files of each letter 3 was given.
fn letters_to_three(realm: &Realm) -> Vec<Vec<ParcelFile>> {
    submits_to(realm, THREE)
        .into_iter()
        .filter_map(|submit| match submit.input {
            Input::Letter { files, .. } => Some(files),
            _ => None,
        })
        .collect()
}

async fn letter_files(
    realm: &Realm,
    count: usize,
) -> Vec<ParcelFile> {
    until(&format!("3 has {count} letters"), || {
        letters_to_three(realm).len() >= count
    })
    .await;
    letters_to_three(realm).remove(count - 1)
}

async fn fetch(
    realm: &Realm,
    by: AgentNumber,
    parcel: ParcelId,
    file: u32,
    part: u32,
) -> Part<PartRefusal> {
    let asked = FetchPart { parcel, file, part };
    realm.tool(by, &asked).await.unwrap()
}

/// The bytes of `file` as 3 fetches it, part by part.
async fn fetched(realm: &Realm, file: &ParcelFile) -> Vec<u8> {
    let mut bytes = Vec::new();
    for part in 0.. {
        match fetch(realm, THREE, file.parcel, file.file, part).await
        {
            Part::Bytes { bytes: more, last } => {
                bytes.extend(more.0);
                if last {
                    return bytes;
                }
            }
            Part::Refused { why } => panic!("part {part}: {why}"),
        }
    }
    unreachable!()
}

fn notices_to_mayor(realm: &Realm) -> Vec<String> {
    submits_to(realm, MAYOR)
        .into_iter()
        .filter_map(|Submit { input, .. }| match input {
            Input::Notice { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

async fn notice_to_mayor(realm: &Realm) -> String {
    until("the mayor is told", || {
        !notices_to_mayor(realm).is_empty()
    })
    .await;
    notices_to_mayor(realm).remove(0)
}

/// The names in the realm's store of files.
fn stored(realm: &Realm) -> Vec<String> {
    let store = realm.scratch.join("actors/realm/local.files");
    let Ok(listed) = std::fs::read_dir(store) else {
        return Vec::new();
    };
    let mut names = listed
        .map(|entry| {
            entry.unwrap().file_name().into_string().unwrap()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn bytes_of(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|n| (n % 251) as u8 ^ seed).collect()
}

/// Answers the chief's wait `wait` with `choice` once it is open,
/// checking first that 3 has no new letter.
async fn chief_answers(
    realm: &Realm,
    wait: &str,
    choice: Choice,
    text: Option<&str>,
    letters_before: usize,
) {
    let begun = std::time::Instant::now();
    loop {
        assert_eq!(
            letters_to_three(realm).len(),
            letters_before,
            "no letter goes before the chief answers"
        );
        let answered = realm
            .ask(&answer(wait, choice.clone(), text))
            .await
            .unwrap();
        match answered {
            Done::Done => return,
            Done::Refused {
                why: AnswerRefusal::NothingWaits { .. },
            } => {}
            other => panic!("the wait answers {other:?}"),
        }
        assert!(begun.elapsed() < super::DEADLINE, "no wait {wait}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Waits until the chief's wait `wait` is open: a reply, which a
/// send's wait does not take, is refused as not offered.
async fn opens(realm: &Realm, wait: &str) {
    let begun = std::time::Instant::now();
    loop {
        let tried = realm
            .ask(&answer(wait, Choice::Reply, Some("no")))
            .await
            .unwrap();
        match tried {
            Done::Refused {
                why: AnswerRefusal::NotOffered,
            } => return,
            Done::Refused {
                why: AnswerRefusal::NothingWaits { .. },
            } => {}
            other => panic!("the wait answers {other:?}"),
        }
        assert!(begun.elapsed() < super::DEADLINE, "no wait {wait}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_parcel_reaches_the_receiver_byte_for_byte() {
    let report = bytes_of(PARCEL_PART * 2 + 100, 7);
    let (realm, _) =
        sending(Files::with(&[("/project/out/report.bin", &report)]))
            .await;

    sends(&realm, "t1", "See attached.", &["/project/out"]).await;
    let files = letter_files(&realm, 1).await;
    let got = fetched(&realm, &files[0]).await;
    let by_sender = fetch(&realm, MAYOR, files[0].parcel, 0, 0).await;
    let past = fetch(&realm, THREE, files[0].parcel, 0, 3).await;

    assert_eq!(
        files,
        [ParcelFile {
            parcel: files[0].parcel,
            file: 0,
            path: "from-hire-2/out/report.bin".into(),
            size: report.len() as u64,
        }]
    );
    assert!(got == report, "the fetched parts make the file");
    assert_eq!(
        by_sender,
        Part::Refused {
            why: PartRefusal::NotYours
        }
    );
    assert_eq!(
        past,
        Part::Refused {
            why: PartRefusal::NoSuchPart
        }
    );
    assert_eq!(stored(&realm), [hash(&report).to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn over_the_cap_a_parcel_waits_for_a_chief() {
    let (realm, files) = sending(over_the_cap()).await;

    sends(&realm, "t1", "Everything.", &["/project/big"]).await;
    opens(&realm, "1").await;
    let seen = super::world::seen_until(
        &realm,
        "the send wait shows",
        |seen| !seen.waits.is_empty(),
    )
    .await;
    let realm = restart(realm, &files).await;
    realm.write("UPDATE parcel SET at = 0 WHERE id = 1").await;
    chief_answers(&realm, "1", Choice::Allow, None, 0).await;
    let sent = letter_files(&realm, 1).await;
    let first = fetched(&realm, &sent[0]).await;
    sends(&realm, "t2", "Again.", &["/project/big"]).await;
    chief_answers(&realm, "2", Choice::Deny, Some("too much"), 1)
        .await;
    let told = notice_to_mayor(&realm).await;
    let subjects = realm
        .stopped_rows(|db| {
            db.prepare("SELECT kind, subject FROM wait ORDER BY id")?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<(String, String)>, _>>()
        })
        .await;

    assert_eq!(sent.len(), 1001);
    assert_eq!(first, b"x");
    assert_eq!(
        sent[1000].path,
        PathBuf::from("from-hire-2/big/1001")
    );
    assert_eq!(told, "the chief refused it: too much");
    assert_eq!(letters_to_three(&realm).len(), 1);
    assert_eq!(subjects.len(), 2);
    for (kind, subject) in &subjects {
        assert_eq!(kind, "send");
        assert!(
            subject.starts_with("1001 bytes in 1001 files: tell "),
            "{subject}"
        );
    }
    assert!(subjects[1].1.contains("Again."));
    assert_eq!(
        seen.waits
            .iter()
            .map(|wait| wait.kind.clone())
            .collect::<Vec<_>>(),
        [WaitKind::Send {
            measure: Measure {
                files: 1001,
                bytes: 1001,
            },
        }]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_parcel_outside_the_reach_is_told_to_the_sender() {
    let (realm, _) =
        sending(Files::with(&[("/project/a", b"a")])).await;

    sends(&realm, "t1", "Here.", &["/outside/secret"]).await;
    let told = notice_to_mayor(&realm).await;

    assert_eq!(
        told,
        "`/outside/secret` cannot be read: /outside/secret is out \
         of this agent's reach to read"
    );
    assert!(letters_to_three(&realm).is_empty());
    assert!(submits_to(&realm, THREE).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_keeps_open_parcels() {
    let slow = bytes_of(PARCEL_PART + 10, 1);
    let (realm, files) = sending(Files::with(&[
        ("/project/a", b"aaa"),
        ("/project/b", b"bbb"),
        ("/project/slow", &slow),
    ]))
    .await;
    sends(&realm, "t1", "A.", &["/project/a"]).await;
    let a = letter_files(&realm, 1).await.remove(0);
    let store = realm.scratch.join("actors/realm/local.files");
    let orphan = hash(b"no one").to_string();
    std::fs::write(store.join(&orphan), "no one").unwrap();
    std::fs::write(store.join("99.0.part"), "stale").unwrap();
    files.hold_from(1);

    sends(&realm, "t2", "Slow.", &["/project/slow"]).await;
    until("the slow copy waits on its second part", || {
        *files.waiting.lock().unwrap()
    })
    .await;
    sends(&realm, "t3", "B.", &["/project/b"]).await;
    let b = letter_files(&realm, 2).await.remove(0);
    until("the store is swept", || {
        !stored(&realm)
            .iter()
            .any(|name| *name == orphan || name == "99.0.part")
    })
    .await;
    let while_copying = stored(&realm);
    files.release();
    let slow_file = letter_files(&realm, 3).await.remove(0);
    let slow_got = fetched(&realm, &slow_file).await;
    std::fs::write(store.join(&orphan), "no one").unwrap();
    let realm = restart(realm, &files).await;
    let after_restart = stored(&realm);
    let a_after_restart = fetched(&realm, &a).await;
    realm.write("UPDATE parcel SET at = 0 WHERE id = 1").await;
    let a_old = fetch(&realm, THREE, a.parcel, 0, 0).await;

    let named = |bytes: &[u8]| hash(bytes).to_string();
    let mut kept = vec![
        named(b"aaa"),
        named(b"bbb"),
        format!("{}.0.part", slow_file.parcel.0),
    ];
    kept.sort();
    assert_eq!(
        while_copying, kept,
        "the open parcels keep their files"
    );
    assert!(slow_got == slow);
    let mut kept = vec![named(b"aaa"), named(b"bbb"), named(&slow)];
    kept.sort();
    assert_eq!(after_restart, kept);
    assert_eq!(a_after_restart, b"aaa");
    assert_eq!(
        a_old,
        Part::Refused {
            why: PartRefusal::NoSuchParcel
        }
    );
    let mut kept = vec![named(b"bbb"), named(&slow)];
    kept.sort();
    assert_eq!(stored(&realm), kept);
    assert_eq!(fetched(&realm, &b).await, b"bbb");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kill_mid_copy_leaves_no_partial_file() {
    let report = bytes_of(PARCEL_PART * 2 + 5, 3);
    let (realm, files) =
        sending(Files::with(&[("/project/report", &report)])).await;
    files.hold_from(1);

    sends(&realm, "t1", "Report.", &["/project/report"]).await;
    until("the copy waits on its second part", || {
        *files.waiting.lock().unwrap()
    })
    .await;
    let killed = stored(&realm);
    let realm = killed_while_held(realm, &files).await;
    let sent = letter_files(&realm, 1).await;
    let got = fetched(&realm, &sent[0]).await;

    assert!(
        !killed.contains(&hash(&report).to_string()),
        "no file by its hash before it is whole: {killed:?}"
    );
    assert!(got == report, "the parts fetched make the file");
    assert_eq!(stored(&realm), [hash(&report).to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_that_changes_while_it_is_copied_is_not_sent() {
    let report = bytes_of(PARCEL_PART + 10, 5);
    let (realm, files) =
        sending(Files::with(&[("/project/report", &report)])).await;
    files.hold_from(0);

    sends(&realm, "t1", "Report.", &["/project/report"]).await;
    until("the copy waits on its first part", || {
        *files.waiting.lock().unwrap()
    })
    .await;
    files.add(&[("/project/report", &bytes_of(report.len(), 9))]);
    files.release();
    let told = notice_to_mayor(&realm).await;
    until("the store is swept", || stored(&realm).is_empty()).await;

    assert_eq!(
        told,
        "`report` cannot be read: it changed while it was sent."
    );
    assert!(letters_to_three(&realm).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_that_shrinks_while_it_is_copied_reads_as_one_that_changed()
 {
    let report = bytes_of(PARCEL_PART + 10, 5);
    let (realm, files) =
        sending(Files::with(&[("/project/report", &report)])).await;
    files.hold_from(1);

    sends(&realm, "t1", "Report.", &["/project/report"]).await;
    until("the copy waits on its second part", || {
        *files.waiting.lock().unwrap()
    })
    .await;
    files.add(&[("/project/report", b"short")]);
    files.release();
    let told = notice_to_mayor(&realm).await;

    assert_eq!(
        told,
        "`report` cannot be read: it changed while it was sent."
    );
    assert!(letters_to_three(&realm).is_empty());
}

/// Mayor 2 tells 3 on `task`, with the files under `paths`.
async fn sends_on(
    realm: &Realm,
    call: &str,
    task: &RealmTask,
    paths: &[&str],
) {
    let sent = Tell {
        paths: paths
            .iter()
            .map(|path| Enclosed { path: path.into() })
            .collect(),
        ..on(task, "On the task.")
    };
    let answer = realm.tool(MAYOR, &tool(call, sent)).await.unwrap();
    assert_eq!(answer, Done::Done, "tell answers at once");
}

/// 3 objects to a chief's new line, and the mayor sends it a file
/// on that line's task. Returns the objection's wait.
async fn held_with_a_file(realm: &Realm) -> String {
    let task = chief_task(realm, THREE).await;
    let objected = realm
        .tool(THREE, &tool("o1", objection(&task, "no")))
        .await
        .unwrap();
    assert_eq!(objected, Done::Done);
    sends_on(realm, "t1", &task, &["/project/a"]).await;
    "1".to_owned()
}

/// The rows of the `held` table.
async fn go(realm: &Realm, wait: &str) {
    let ruled =
        realm.ask(&answer(wait, Choice::Go, None)).await.unwrap();
    assert_eq!(ruled, Done::Done, "go on wait {wait}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_letters_files_wait_for_go() {
    let (realm, _) =
        sending(Files::with(&[("/project/a", b"aaa")])).await;
    let wait = held_with_a_file(&realm).await;

    let held = held_rows(&realm).await;
    let letters_while_held = letters_to_three(&realm).len();
    let early = fetch(&realm, THREE, ParcelId(1), 0, 0).await;
    go(&realm, &wait).await;
    let files = letter_files(&realm, 1).await;

    assert_eq!((held, letters_while_held), (1, 0));
    assert_eq!(
        early,
        Part::Refused {
            why: PartRefusal::NoSuchParcel
        }
    );
    assert_eq!(fetched(&realm, &files[0]).await, b"aaa");
    assert_eq!(stored(&realm), [hash(b"aaa").to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_letters_files_outlive_a_day() {
    let (realm, _) =
        sending(Files::with(&[("/project/a", b"aaa")])).await;
    let wait = held_with_a_file(&realm).await;
    held_rows(&realm).await;

    realm.write("UPDATE parcel SET at = 0 WHERE id = 1").await;
    let after_a_day = stored(&realm);
    go(&realm, &wait).await;
    let files = letter_files(&realm, 1).await;
    held_rows(&realm).await;

    assert_eq!(after_a_day, [hash(b"aaa").to_string()]);
    assert_eq!(fetched(&realm, &files[0]).await, b"aaa");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kill_after_ready_keeps_a_held_letter() {
    let (realm, _) =
        sending(Files::with(&[("/project/a", b"aaa")])).await;
    let task = chief_task(&realm, THREE).await;
    sends_on(&realm, "t1", &task, &["/project/a"]).await;
    letter_files(&realm, 1).await;
    realm
        .tool(THREE, &tool("o1", objection(&task, "no")))
        .await
        .unwrap();
    realm.seen.submits.lock().unwrap().retain(|(to, submit)| {
        !(*to == THREE
            && matches!(submit.input, Input::Letter { .. }))
    });

    realm
        .write(
            "UPDATE step SET done = 'copy', closed = 0 WHERE kind = \
             'parcel' AND n = 1",
        )
        .await;
    let held = held_rows(&realm).await;
    let letters_while_held = letters_to_three(&realm).len();
    go(&realm, "1").await;
    letter_files(&realm, 1).await;

    assert_eq!((held, letters_while_held), (1, 0));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_receiver_fetches_nothing() {
    let (realm, _) =
        sending(Files::with(&[("/project/a", b"aaa")])).await;
    sends(&realm, "t1", "A.", &["/project/a"]).await;
    let files = letter_files(&realm, 1).await;
    realm
        .write(
            "UPDATE agent SET status = 'ended', ended_how = \
             '{\"type\":\"by_the_chief\"}' WHERE id = 3",
        )
        .await;

    let ended = fetch(&realm, THREE, files[0].parcel, 0, 0).await;

    assert_eq!(
        ended,
        Part::Refused {
            why: PartRefusal::NoSuchParcel
        }
    );
}

/// Each parcel's state, and whether it is swept.
async fn parcel_states(realm: &Realm) -> Vec<(String, bool)> {
    realm
        .stopped_rows(|db| {
            db.prepare(
                "SELECT state, swept_at IS NOT NULL FROM parcel \
                 ORDER BY id",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect()
        })
        .await
}

fn over_the_cap() -> Files {
    let names = (1..=1001)
        .map(|n| format!("/project/big/{n:04}"))
        .collect::<Vec<_>>();
    let files = names
        .iter()
        .map(|name| (name.as_str(), &b"x"[..]))
        .collect::<Vec<_>>();
    Files::with(&files)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lapsed_send_wait_ends_its_parcel() {
    let (realm, files) = sending(over_the_cap()).await;
    sends(&realm, "t1", "Everything.", &["/project/big"]).await;
    opens(&realm, "1").await;

    let lapse = Lapse {
        conversation: None,
        why: LapseReason::Stopped,
    };
    realm.tool(MAYOR, &lapse).await.unwrap();
    let realm = restart(realm, &files).await;
    let states = parcel_states(&realm).await;
    let allowed =
        realm.ask(&answer("1", Choice::Allow, None)).await.unwrap();

    assert_eq!(states, [("refused".to_owned(), true)]);
    assert!(
        matches!(
            allowed,
            Done::Refused {
                why: AnswerRefusal::NothingWaits { .. }
            }
        ),
        "{allowed:?}"
    );
    assert!(letters_to_three(&realm).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_parcel_whose_sender_ended_is_swept() {
    let (realm, _) = sending(over_the_cap()).await;
    sends(&realm, "t1", "Everything.", &["/project/big"]).await;
    opens(&realm, "1").await;

    realm
        .write(
            "UPDATE agent SET status = 'ended', ended_how = \
             '{\"type\":\"by_the_chief\"}' WHERE id = 2",
        )
        .await;
    let states = parcel_states(&realm).await;

    assert_eq!(states, [("refused".to_owned(), true)]);
}
