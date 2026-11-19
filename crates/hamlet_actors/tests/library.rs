//! The library through a host: a realm stand-in and agent stand-ins
//! call it as themselves.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory, ShutdownKind};
use hamlet_actor_error::ActorError;
use hamlet_actors::library;
use hamlet_host::{Address, Caller, Host, HostOptions, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::bytes::{Base64, Sha256};
use hamlet_messages::library::{
    BadOffer, BadSkill, DropCopy, NoOffer, NoSkill, Offer,
    OfferAnswer, OfferId, OfferRead, OfferRefusal, OnTheShelves,
    ReadOffer, ReadShelves, ShelfList, Shelves, ShelvesChanged,
    Skill, Unshelve, Verdict, VerdictRefusal,
};
use hamlet_messages::profile::{By, Piece, TooLarge};
use hamlet_messages::realm::Done;
use hamlet_messages::workspace::{AgentNumber, CallId};
use hamlet_messages::{MOST_PART, PIECE_FRAME};
use hamlet_protocol::client::RequestId;
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

const STARTERS: [&str; 8] = [
    "call-api",
    "manage-memory",
    "read-webpage",
    "system-info",
    "weather",
    "web-search",
    "write-skill",
    "write-typst",
];

const PAPER: &str = "---\nname: tidy\ndescription: Tidy a \
                     folder.\nusage: python3 \
                     skills/tidy/tidy.py\n---\nTidy.\n";

const SCRIPT: &str = "print('tidy')\n";

/// The messages the agent stand-ins were told, each under its
/// address.
#[derive(Default)]
struct Told {
    shelves: Mutex<Vec<(String, ShelfList)>>,
    drops: Mutex<Vec<(String, DropCopy)>>,
}

/// Stands in for the realm and the agents: calls the library as
/// itself, and keeps what it is told.
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
    type Actions = (Forward, ShelvesChanged, DropCopy);
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
                &library_at(),
                &forward.name,
                forward.args,
                Duration::from_secs(20),
            )
            .await
    }
}

impl Handler<ShelvesChanged> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        changed: ShelvesChanged,
    ) -> anyhow::Result<()> {
        let mut told = self.told.shelves.lock().unwrap();
        told.push((ctx.actor_id().to_owned(), changed.list));
        Ok(())
    }
}

impl Handler<DropCopy> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        drop: DropCopy,
    ) -> anyhow::Result<()> {
        let mut told = self.told.drops.lock().unwrap();
        told.push((ctx.actor_id().to_owned(), drop));
        Ok(())
    }
}

fn peer(kind: &str) -> ActorFactory {
    let mut registry = Registry::new();
    registry
        .register_actor_with::<Peer>(kind, ActorConfig::default());
    registry.into_factories().remove(kind).unwrap()
}

fn library_at() -> Address {
    address::Library.address().unwrap()
}

fn realm() -> Address {
    address::Realm.address().unwrap()
}

fn agent(n: u32) -> Address {
    address::Agent(AgentNumber(n)).address().unwrap()
}

fn open_with(dir: &Path, told: Arc<Told>) -> Host {
    let mut kinds = HashMap::from([(
        address::Library::KIND.to_owned(),
        library::factory(),
    )]);
    for kind in [address::Realm::KIND, address::Agent::KIND] {
        kinds.insert(kind.to_owned(), peer(kind));
    }
    Host::open_with(
        dir,
        kinds,
        HostOptions::default().with_value(told),
    )
    .unwrap()
}

async fn open(dir: &Path, told: Arc<Told>) -> Host {
    let host = open_with(dir, told);
    host.get_or_start(&library_at()).await.unwrap();
    host
}

async fn call_as<A: Action>(
    host: &Host,
    from: &Address,
    asked: &A,
) -> anyhow::Result<A::Output> {
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
    let bytes: Vec<u8> = ciborium::from_reader(reply.as_slice())
        .context("the peer's reply")?;
    Ok(ciborium::from_reader(bytes.as_slice())?)
}

fn error_code<T: std::fmt::Debug>(
    reply: anyhow::Result<T>,
) -> String {
    let error = ActorError::extract(&reply.unwrap_err());
    format!("{}.{}", error.group(), error.code())
}

fn repository(dir: &Path) -> hamlet_git::Repository {
    hamlet_git::Repository::open(&dir.join("library.git")).unwrap()
}

fn main_tip(dir: &Path) -> hamlet_git::CommitId {
    repository(dir).tip(hamlet_git::MAIN).unwrap().unwrap()
}

/// The skills `library/` holds, by their folders.
fn checked_out(dir: &Path) -> Vec<String> {
    let mut skills = std::fs::read_dir(dir.join("library"))
        .unwrap()
        .map(|entry| {
            entry.unwrap().file_name().into_string().unwrap()
        })
        .collect::<Vec<_>>();
    skills.sort();
    skills
}

fn names(list: &ShelfList) -> Vec<&str> {
    list.skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect()
}

fn piece(path: &str, offset: u32, bytes: &[u8], last: bool) -> Piece {
    Piece {
        path: path.into(),
        offset,
        bytes: Base64(bytes.to_vec()),
        last,
        executable: false,
    }
}

fn offer(
    call: &str,
    skill: &str,
    part: u32,
    last: bool,
    files: Vec<Piece>,
) -> Offer {
    Offer {
        call: CallId(call.into()),
        skill: skill.into(),
        reason: "folders get messy".into(),
        part,
        last,
        files,
    }
}

/// The skill `tidy`, offered whole in one part.
fn tidy(call: &str) -> Offer {
    offer(
        call,
        "tidy",
        0,
        true,
        vec![
            piece("SKILL.md", 0, PAPER.as_bytes(), true),
            piece("tidy.py", 0, SCRIPT.as_bytes(), true),
        ],
    )
}

/// Offers `asked` as agent `n` and gives the offer it was kept as.
async fn offered(host: &Host, n: u32, asked: &Offer) -> OfferId {
    match call_as(host, &agent(n), asked).await.unwrap() {
        OfferAnswer::Kept { offer } => offer,
        other => panic!("{other:?}"),
    }
}

fn verdict(offer: OfferId, accepted: bool) -> Verdict {
    Verdict {
        id: RequestId::new(),
        offer,
        accepted,
        words: "The librarian's words.".into(),
    }
}

fn unshelve(skill: &str) -> Unshelve {
    Unshelve {
        id: RequestId::new(),
        skill: skill.into(),
        reason: Some("no longer needed".into()),
        by: By::Chief { chief: 3 },
    }
}

async fn read_shelves(host: &Host) -> ShelfList {
    call_as(host, &realm(), &ReadShelves).await.unwrap()
}

async fn until(what: &str, done: impl Fn() -> bool) {
    let waited =
        tokio::time::timeout(Duration::from_secs(5), async {
            while !done() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
    waited.await.unwrap_or_else(|_| panic!("{what} within 5s"));
}

/// The library's `shelf_event` rows, `(kind, skill, by, why)`, read
/// once its host is shut down.
fn events(dir: &Path) -> Vec<(String, String, String, String)> {
    let db = Connection::open_with_flags(
        dir.join("actors/library/local.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut read = db
        .prepare(
            "SELECT kind, skill, by, why FROM shelf_event ORDER BY \
             id",
        )
        .unwrap();
    read.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_first_start_writes_the_starter_skills_and_a_second_leaves_them()
 {
    let dir = Scratch::new("library-start");
    let host = open(&dir, Arc::default()).await;
    let made = main_tip(&dir);
    let first = repository(&dir).history(made, 1).unwrap().remove(0);
    assert_eq!(
        (first.message.as_str(), first.author.as_str()),
        ("The starter skills", "realm")
    );
    assert_eq!(checked_out(&dir), STARTERS);
    let script = dir.join("library/web-search/scripts/search.py");
    assert!(
        std::fs::read_to_string(&script).unwrap().contains("def ")
    );
    host.shutdown().await.unwrap();
    drop(host);
    std::fs::remove_dir_all(dir.join("library/weather")).unwrap();

    let host = open(&dir, Arc::default()).await;
    assert_eq!(main_tip(&dir), made);
    assert_eq!(repository(&dir).history(made, 10).unwrap().len(), 1);
    assert_eq!(checked_out(&dir), STARTERS);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_shelves_are_listed_with_their_version() {
    let dir = Scratch::new("library-shelves");
    let host = open(&dir, Arc::default()).await;
    let asked = Shelves { version_seen: None };
    let list = call_as(&host, &agent(7), &asked).await.unwrap();
    assert_eq!(list.version, 1);
    assert_eq!(names(&list), STARTERS);
    let weather = Skill {
        name: "weather".into(),
        description: "Current weather and a three-day forecast for \
                      a place, in metric or US units."
            .into(),
        usage: "python3 skills/weather/scripts/weather.py \
                \"Toronto\" [metric|us]"
            .into(),
    };
    assert!(list.skills.contains(&weather));
    let memory = &list.skills[1];
    assert_eq!(
        (memory.name.as_str(), memory.usage.as_str()),
        ("manage-memory", "")
    );
    assert_eq!(read_shelves(&host).await, list);
    let again = Shelves {
        version_seen: Some(1),
    };
    assert_eq!(
        call_as(&host, &agent(7), &again).await.unwrap(),
        list
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_offer_is_shelved_and_its_offerer_drops_its_copy()
{
    let dir = Scratch::new("library-accepted");
    let told = Arc::new(Told::default());
    let host = open(&dir, told.clone()).await;
    let listening = Shelves { version_seen: None };
    call_as(&host, &agent(8), &listening).await.unwrap();
    call_as(&host, &agent(8), &listening).await.unwrap();

    let (head, tail) = PAPER.as_bytes().split_at(10);
    let first = offer(
        "c1",
        "tidy",
        0,
        false,
        vec![piece("SKILL.md", 0, head, false)],
    );
    let second = offer(
        "c1",
        "tidy",
        1,
        true,
        vec![
            piece("SKILL.md", 10, tail, true),
            piece("tidy.py", 0, SCRIPT.as_bytes(), true),
        ],
    );
    let more = call_as(&host, &agent(7), &first).await.unwrap();
    assert_eq!(more, OfferAnswer::More);
    assert_eq!(
        call_as(&host, &agent(7), &first).await.unwrap(),
        more
    );
    let kept = offered(&host, 7, &second).await;
    assert_eq!(offered(&host, 7, &second).await, kept);
    assert_eq!(read_shelves(&host).await.version, 1);

    let judged = verdict(kept, true);
    let done = call_as(&host, &realm(), &judged).await.unwrap();
    assert_eq!(done, Done::Done);
    assert_eq!(
        call_as(&host, &realm(), &judged).await.unwrap(),
        done
    );
    let shelved = main_tip(&dir);
    assert_eq!(
        repository(&dir).history(shelved, 10).unwrap().len(),
        2
    );
    let paper =
        std::fs::read_to_string(dir.join("library/tidy/SKILL.md"));
    assert_eq!(paper.unwrap(), PAPER);
    let list = read_shelves(&host).await;
    assert_eq!(list.version, 2);
    assert!(names(&list).contains(&"tidy"));

    until("the listener and the offerer told", || {
        !told.shelves.lock().unwrap().is_empty()
            && !told.drops.lock().unwrap().is_empty()
    })
    .await;
    let shelves = told.shelves.lock().unwrap().clone();
    assert_eq!(shelves, [(agent(8).to_string(), list)]);
    let mut digest = sha2::Sha256::new();
    for (path, bytes) in [("SKILL.md", PAPER), ("tidy.py", SCRIPT)] {
        for part in [path, bytes] {
            digest.update((part.len() as u64).to_be_bytes());
            digest.update(part);
        }
    }
    let copy = DropCopy {
        skill: "tidy".into(),
        digest: Sha256(digest.finalize().into()),
    };
    let drops = told.drops.lock().unwrap().clone();
    assert_eq!(drops, [(agent(7).to_string(), copy)]);
    host.shutdown().await.unwrap();
    assert_eq!(
        events(&dir),
        [(
            "shelved".to_owned(),
            "tidy".to_owned(),
            "agent:7".to_owned(),
            "folders get messy".to_owned()
        )]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_offer_shelves_nothing_and_is_judged_once() {
    let dir = Scratch::new("library-refused");
    let host = open(&dir, Arc::default()).await;
    let before = read_shelves(&host).await;
    let kept = offered(&host, 7, &tidy("c1")).await;
    let judged = verdict(kept, false);
    let done = call_as(&host, &realm(), &judged).await.unwrap();
    assert_eq!(done, Done::Done);
    assert_eq!(
        call_as(&host, &realm(), &judged).await.unwrap(),
        done
    );
    assert_eq!(read_shelves(&host).await, before);
    assert!(!dir.join("library/tidy").exists());

    let again = verdict(kept, true);
    assert_eq!(
        call_as(&host, &realm(), &again).await.unwrap(),
        Done::Refused {
            why: VerdictRefusal::Judged { offer: kept }
        }
    );
    let none = verdict(OfferId(99), true);
    assert_eq!(
        call_as(&host, &realm(), &none).await.unwrap(),
        Done::Refused {
            why: VerdictRefusal::NoSuchOffer { offer: OfferId(99) }
        }
    );
    // A refused offer is not judged any more: another may come.
    offered(&host, 8, &tidy("c2")).await;
    host.shutdown().await.unwrap();
    assert_eq!(
        events(&dir),
        [(
            "refused".to_owned(),
            "tidy".to_owned(),
            "agent:7".to_owned(),
            "The librarian's words.".to_owned()
        )]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unshelved_skill_leaves_library_and_the_list() {
    let dir = Scratch::new("library-unshelve");
    let told = Arc::new(Told::default());
    let host = open(&dir, told.clone()).await;
    let listening = Shelves { version_seen: None };
    call_as(&host, &agent(8), &listening).await.unwrap();

    let asked = unshelve("weather");
    let done = call_as(&host, &realm(), &asked).await.unwrap();
    assert_eq!(done, Done::Done);
    assert_eq!(call_as(&host, &realm(), &asked).await.unwrap(), done);
    assert!(!dir.join("library/weather").exists());
    let list = read_shelves(&host).await;
    assert_eq!(list.version, 2);
    assert!(!names(&list).contains(&"weather"));
    until("the listener told", || {
        !told.shelves.lock().unwrap().is_empty()
    })
    .await;
    let shelves = told.shelves.lock().unwrap().clone();
    assert_eq!(shelves, [(agent(8).to_string(), list)]);

    for skill in ["weather", "web-search/scripts", "", "nothing"] {
        let refused =
            call_as(&host, &realm(), &unshelve(skill)).await;
        assert_eq!(
            refused.unwrap(),
            Done::Refused {
                why: NoSkill::NoSuchSkill {
                    skill: skill.into()
                }
            }
        );
    }
    assert!(
        dir.join("library/web-search/scripts/search.py").exists()
    );
    host.shutdown().await.unwrap();
    assert_eq!(
        events(&dir),
        [(
            "unshelved".to_owned(),
            "weather".to_owned(),
            "chief:3".to_owned(),
            "no longer needed".to_owned()
        )]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offer_is_refused_for_each_reason() {
    let dir = Scratch::new("library-offer-refusals");
    let host = open(&dir, Arc::default()).await;
    let refused = async |n: u32, asked: Offer| match call_as(
        &host,
        &agent(n),
        &asked,
    )
    .await
    .unwrap()
    {
        OfferAnswer::Refused { why } => why,
        other => panic!("{other:?}"),
    };

    for skill in ["../tidy", "a/b", ""] {
        let mut asked = tidy("bad-name");
        asked.skill = skill.into();
        assert_eq!(
            refused(7, asked).await,
            OfferRefusal::Shape(BadOffer::NotASkillName {
                skill: skill.into()
            })
        );
    }

    let long = "n".repeat(256);
    for path in ["a\\b.py", "/abs.py", "a//b.py", "", long.as_str()] {
        let asked = offer(
            "bad-path",
            "tidy",
            0,
            true,
            vec![
                piece("SKILL.md", 0, PAPER.as_bytes(), true),
                piece(path, 0, SCRIPT.as_bytes(), true),
            ],
        );
        assert_eq!(
            refused(7, asked).await,
            OfferRefusal::Shape(BadOffer::NotAPath {
                path: path.into()
            })
        );
    }

    let path = "big.bin";
    let over = MOST_PART - path.len() - PIECE_FRAME + 1;
    let big = offer(
        "big-part",
        "tidy",
        0,
        false,
        vec![piece(path, 0, &vec![0; over], false)],
    );
    assert_eq!(
        refused(7, big).await,
        OfferRefusal::Size(TooLarge::PartTooLarge {
            most: u32::try_from(MOST_PART).unwrap()
        })
    );

    let mut weather = tidy("shelved");
    weather.skill = "weather".into();
    assert_eq!(
        refused(7, weather).await,
        OfferRefusal::Shelves(OnTheShelves::AlreadyShelved {
            skill: "weather".into()
        })
    );

    let bare = offer(
        "bare",
        "tidy",
        0,
        true,
        vec![piece("SKILL.md", 0, b"---\nname: tidy\n---\n", true)],
    );
    assert_eq!(
        refused(7, bare).await,
        OfferRefusal::Skill(BadSkill::NoFrontMatter {
            skill: "tidy".into()
        })
    );

    // Six parts of 700 KiB: the sixth takes the skill over 4 MiB.
    let size = 700 * 1024;
    let mut answers = Vec::new();
    for part in 0..6 {
        let offset = part * u32::try_from(size).unwrap();
        let asked = offer(
            "large",
            "large",
            part,
            false,
            vec![piece("large.bin", offset, &vec![0; size], false)],
        );
        answers
            .push(call_as(&host, &agent(7), &asked).await.unwrap());
    }
    assert_eq!(answers[..5], [const { OfferAnswer::More }; 5]);
    assert_eq!(
        answers[5],
        OfferAnswer::Refused {
            why: OfferRefusal::Skill(BadSkill::SkillTooLarge {
                skill: "large".into()
            })
        }
    );

    let kept = offered(&host, 7, &tidy("whole")).await;
    assert_eq!(
        refused(8, tidy("judged")).await,
        OfferRefusal::Shelves(OnTheShelves::BeingJudged {
            skill: "tidy".into()
        })
    );
    assert_eq!(
        refused(7, tidy("again")).await,
        OfferRefusal::Shelves(OnTheShelves::BeingJudged {
            skill: "tidy".into()
        })
    );
    assert_eq!(
        call_as(&host, &realm(), &verdict(kept, true))
            .await
            .unwrap(),
        Done::Done
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offer_s_last_part_keeps_it_only_when_every_part_before_it_came()
 {
    let dir = Scratch::new("library-offer-order");
    let host = open(&dir, Arc::default()).await;
    let part = |call: &str, part: u32, last: bool, path: &str| {
        let bytes = match path {
            "SKILL.md" => PAPER.as_bytes(),
            _ => SCRIPT.as_bytes(),
        };
        offer(
            call,
            "tidy",
            part,
            last,
            vec![piece(path, 0, bytes, true)],
        )
    };
    let more = async |asked: Offer| {
        let answer = call_as(&host, &agent(7), &asked).await.unwrap();
        assert_eq!(answer, OfferAnswer::More);
    };

    more(part("o1", 0, false, "SKILL.md")).await;
    call_as(&host, &agent(7), &part("o1", 2, true, "c.py"))
        .await
        .unwrap_err();
    more(part("o2", 0, false, "SKILL.md")).await;
    let answer =
        call_as(&host, &agent(7), &part("o2", 1, false, "/b.py"))
            .await
            .unwrap();
    assert!(
        matches!(answer, OfferAnswer::Refused { .. }),
        "{answer:?}"
    );
    call_as(&host, &agent(7), &part("o2", 2, true, "c.py"))
        .await
        .unwrap_err();

    more(part("o3", 0, false, "SKILL.md")).await;
    host.stop(&library_at(), ShutdownKind::Sleep).await.unwrap();
    more(part("o3", 1, false, "b.py")).await;
    let kept = offered(&host, 7, &part("o3", 2, true, "c.py")).await;
    let offers = repository(&dir);
    let at =
        offers.tip(&format!("offers/{}", kept.0)).unwrap().unwrap();
    let paths = offers.files(at).unwrap();
    let paths =
        paths.iter().map(|(path, _)| path).collect::<Vec<_>>();
    assert_eq!(paths, ["SKILL.md", "b.py", "c.py"]);
    let made = offers.history(at, 1).unwrap().remove(0);
    assert_eq!(
        (made.message.as_str(), made.author.as_str()),
        ("Offered", "realm")
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_skill_of_many_empty_files_counts_their_paths_toward_its_size()
 {
    let dir = Scratch::new("library-many-empty-files");
    let host = open(&dir, Arc::default()).await;
    let name = "n".repeat(200);
    let mut refused = None;
    for part in 0..8 {
        let files = (0..2500)
            .map(|n| {
                piece(
                    &format!("empty/{name}{part}-{n}.md"),
                    0,
                    b"",
                    true,
                )
            })
            .collect();
        let asked = offer("empties", "empties", part, false, files);
        if let OfferAnswer::Refused { why } =
            call_as(&host, &agent(7), &asked).await.unwrap()
        {
            refused = Some(why);
            break;
        }
    }
    assert_eq!(
        refused,
        Some(OfferRefusal::Skill(BadSkill::SkillTooLarge {
            skill: "empties".into()
        }))
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offer_is_read_once_its_last_part_came() {
    let dir = Scratch::new("library-read-offer");
    let host = open(&dir, Arc::default()).await;
    let kept = offered(&host, 7, &tidy("c1")).await;
    let started = offer(
        "c2",
        "sweep",
        0,
        false,
        vec![piece("SKILL.md", 0, PAPER.as_bytes(), true)],
    );
    assert_eq!(
        call_as(&host, &agent(8), &started).await.unwrap(),
        OfferAnswer::More
    );
    let read = async |offer| {
        call_as(&host, &realm(), &ReadOffer { offer })
            .await
            .unwrap()
    };

    assert_eq!(
        read(kept).await,
        OfferRead::Offered {
            agent: AgentNumber(7),
            skill: "tidy".into(),
            reason: "folders get messy".into(),
            paths: vec!["SKILL.md".into(), "tidy.py".into()],
            skill_md: PAPER.into(),
        }
    );
    let unfinished = OfferId(kept.0 + 1);
    assert_eq!(
        read(unfinished).await,
        OfferRead::Refused {
            why: NoOffer::NoSuchOffer { offer: unfinished }
        }
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn each_action_refuses_a_caller_of_the_other_kind() {
    let dir = Scratch::new("library-caller");
    let host = open(&dir, Arc::default()).await;
    let shelves = Shelves { version_seen: None };
    assert_eq!(
        error_code(call_as(&host, &realm(), &shelves).await),
        "message.wrong_caller"
    );
    assert_eq!(
        error_code(call_as(&host, &realm(), &tidy("c1")).await),
        "message.wrong_caller"
    );
    for asked in [
        call_as(&host, &agent(7), &ReadShelves).await.map(drop),
        call_as(&host, &agent(7), &ReadOffer { offer: OfferId(1) })
            .await
            .map(drop),
        call_as(&host, &agent(7), &verdict(OfferId(1), true))
            .await
            .map(drop),
        call_as(&host, &agent(7), &unshelve("weather"))
            .await
            .map(drop),
    ] {
        assert_eq!(error_code(asked), "message.wrong_caller");
    }
    assert!(dir.join("library/weather").exists());
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_verdict_s_and_an_unshelve_s_repeats_are_answered_from_their_markers()
 {
    let dir = Scratch::new("library-repeat");
    let host = open(&dir, Arc::default()).await;
    let kept = offered(&host, 7, &tidy("c1")).await;
    let judged = verdict(kept, true);
    let done = call_as(&host, &realm(), &judged).await.unwrap();
    assert_eq!(done, Done::Done);
    let removed = Unshelve {
        skill: "tidy".into(),
        ..unshelve("")
    };
    assert_eq!(
        call_as(&host, &realm(), &removed).await.unwrap(),
        Done::Done
    );
    let shelves = read_shelves(&host).await;
    host.shutdown().await.unwrap();
    drop(host);

    let host = open(&dir, Arc::default()).await;
    assert_eq!(
        call_as(&host, &realm(), &judged).await.unwrap(),
        done
    );
    assert_eq!(
        call_as(&host, &realm(), &removed).await.unwrap(),
        Done::Done
    );
    let other = Verdict {
        accepted: false,
        ..judged
    };
    assert_eq!(
        error_code(call_as(&host, &realm(), &other).await),
        "message.id_reused"
    );
    let other = Unshelve {
        skill: "weather".into(),
        ..removed
    };
    assert_eq!(
        error_code(call_as(&host, &realm(), &other).await),
        "message.id_reused"
    );
    assert_eq!(read_shelves(&host).await, shelves);
    assert!(dir.join("library/weather").exists());
    assert!(!dir.join("library/tidy").exists());
    host.shutdown().await.unwrap();
}
