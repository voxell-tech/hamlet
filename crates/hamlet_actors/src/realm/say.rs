//! A chief's line to an agent, with the images and files sent before
//! it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::agent::{
    Input, InputId, LineFile, LineImage, Submit,
};
use hamlet_messages::bytes::Sha256;
use hamlet_messages::realm::Refusable;
use hamlet_messages::realm::client::{Say, SayAnswer, SayRefusal};
use hamlet_messages::realm::parcels::{ParcelFile, ParcelId};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::{AgentNumber, ParcelEntry};
use hamlet_os::fs::TreePath;
use hamlet_protocol::client::{
    self as protocol, AttachedFile, AttachedImage,
};
use hamlet_workspace_wire::image::Format;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt as _;

use super::act::{Act, Decided, Decision};
use super::calls::{Calls, Delivery, Made};
use super::change::{self, Rows};
use super::parcels::store::Store;
use super::tasks::{self, TaskId};
use super::tree::{self, Lineage, Status};
use super::{Realm, chiefs, marker, parcels, reviews};

/// A chief's line, its text scrubbed, with what the store holds of
/// its images and files, or why it is refused.
#[derive(Serialize)]
pub(super) struct Lined {
    say: Say,
    attached: Result<Attached, SayRefusal>,
}

/// A line's images, then its files, each with the parcel's entry that
/// keeps it under the agent's `inbox/`.
#[derive(Clone, Debug, Default, Serialize)]
struct Attached {
    images: Vec<(AttachedImage, ParcelEntry)>,
    files: Vec<(AttachedFile, ParcelEntry)>,
}

impl Lined {
    /// `say` with the realm's secrets scrubbed from its text, and its
    /// images and files read from the store, unless the line under
    /// `key` was answered already.
    pub(super) async fn of(
        realm: &Realm,
        ctx: &Ctx<Realm>,
        key: &marker::Key,
        mut say: Say,
    ) -> Result<Self> {
        let scrub = |text: &str| realm.secrets.scrubbed(text);
        let rows = Rows::outside(ctx)?;
        if marker::find::<IgnoredAny>(&rows, key).await?.is_some() {
            say.0.text = scrub(&say.0.text);
            return Ok(Self {
                say,
                attached: Ok(Attached::default()),
            });
        }
        let attached = match placed(&mut say.0, scrub) {
            Ok(()) => stored(&Store::of(ctx)?, &say.0).await?,
            Err(why) => Err(why),
        };
        Ok(Self { say, attached })
    }
}

/// Refuses a place outside `say`'s text, then scrubs the text with
/// `scrub` a piece at a time between the places, so each place moves
/// with the words before it.
fn placed(
    say: &mut protocol::Say,
    scrub: impl Fn(&str) -> String,
) -> Result<(), SayRefusal> {
    let text = &say.text;
    let mut cuts = Vec::new();
    let places = say
        .images
        .iter()
        .filter_map(|image| image.at)
        .chain(say.files.iter().filter_map(|file| file.at));
    for at in places {
        let place = usize::try_from(at).ok().filter(|&at| {
            at <= text.len() && text.is_char_boundary(at)
        });
        let Some(place) = place else {
            return Err(SayRefusal::NotAPlace {
                at,
                length: text.len() as u64,
            });
        };
        cuts.push(place);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut scrubbed = String::new();
    let mut moved = BTreeMap::new();
    let mut from = 0;
    for cut in cuts {
        scrubbed.push_str(&scrub(&text[from..cut]));
        moved.insert(cut as u64, scrubbed.len() as u64);
        from = cut;
    }
    scrubbed.push_str(&scrub(&text[from..]));
    let places = say
        .images
        .iter_mut()
        .filter_map(|image| image.at.as_mut())
        .chain(
            say.files.iter_mut().filter_map(|file| file.at.as_mut()),
        );
    for at in places {
        *at = moved[at];
    }
    say.text = scrubbed;
    Ok(())
}

/// What `store` holds of `say`'s files, then of its images, each in
/// the order of its place, checked as today's server checks them.
async fn stored(
    store: &Store,
    say: &protocol::Say,
) -> Result<Result<Attached, SayRefusal>> {
    let mut files = say.files.clone();
    files.sort_by_key(|file| file.at.unwrap_or(u64::MAX));
    let mut images = say.images.clone();
    images.sort_by_key(|image| image.at.unwrap_or(u64::MAX));
    let mut kept = Vec::new();
    let mut total = 0_u64;
    for file in files {
        let sha256 = hash(file.id.as_str())?;
        let Some((size, _)) = read(store, &sha256).await? else {
            return Ok(Err(SayRefusal::FileNotSent {
                name: file.name,
            }));
        };
        if size != file.size {
            return Ok(Err(SayRefusal::NotItsSize {
                name: file.name,
            }));
        }
        total += size;
        kept.push((file, sha256, size));
    }
    if total > AttachedFile::MOST_BYTES {
        return Ok(Err(SayRefusal::TooManyBytes));
    }
    let mut attached = Attached::default();
    for image in images {
        let sha256 = hash(image.id.as_str())?;
        let read = read(store, &sha256).await?;
        let Some((size, Some(extension))) =
            read.map(|(size, head)| (size, extension(&head)))
        else {
            return Ok(Err(SayRefusal::NoImage {
                image: image.id.to_string(),
            }));
        };
        let path =
            PathBuf::from(format!("images/{sha256}.{extension}"));
        attached.images.push((image, entry(path, size, sha256)));
    }
    for (file, sha256, size) in kept {
        if TreePath::parse(&file.name).is_err() {
            return Ok(Err(SayRefusal::NotAFileName {
                name: file.name,
            }));
        }
        let path = PathBuf::from(&file.name);
        attached.files.push((file, entry(path, size, sha256)));
    }
    Ok(Ok(attached))
}

fn entry(path: PathBuf, size: u64, sha256: Sha256) -> ParcelEntry {
    ParcelEntry {
        name: path.clone(),
        path,
        size,
        sha256,
    }
}

/// An attachment's id, the SHA-256 of its bytes in hexadecimal.
fn hash(id: &str) -> Result<Sha256> {
    Sha256::from_hex(id).ok_or_else(|| {
        anyhow::anyhow!("the attachment id `{id}` is not a SHA-256")
    })
}

/// The size of the file the store keeps for `sha256` and its first
/// bytes, or `None` when it keeps none.
async fn read(
    store: &Store,
    sha256: &Sha256,
) -> Result<Option<(u64, Vec<u8>)>> {
    let file = match tokio::fs::File::open(store.file(sha256)).await {
        Ok(file) => file,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let size = file.metadata().await?.len();
    let mut head = Vec::new();
    file.take(16).read_to_end(&mut head).await?;
    Ok(Some((size, head)))
}

/// The extension today's server keeps an image in, for the image
/// `bytes` begin as.
fn extension(bytes: &[u8]) -> Option<&'static str> {
    Format::detect(bytes).map(Format::extension)
}

pub(super) struct Target {
    lineage: Option<Lineage>,
    task: TaskId,
    chief: String,
    /// The line's parcel and what the agent is told of its files.
    parcel: Option<(change::Change, Vec<LineImage>, Vec<LineFile>)>,
    /// The changes that hold the line for its hire's lead to review.
    review: Option<Vec<change::Change>>,
}

/// A line's task, and whether the line waits for a review.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) enum Said {
    Line {
        task: TaskId,
        chief: String,
        images: Vec<LineImage>,
        files: Vec<LineFile>,
    },
    Review(TaskId),
}

impl Act<Client> for Lined {
    type Output = SayAnswer;
    type Loaded = Target;
    type Done = Said;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Target> {
        let say = &self.say;
        let lineage = match to(say) {
            Some(to) => tree::lineage(rows, to).await?,
            None => None,
        };
        let task = tasks::fresh(rows).await?;
        let chief = chiefs::name(rows).await?;
        let parcel = match (&lineage, &self.attached) {
            (Some(lineage), Ok(attached))
                if !attached.images.is_empty()
                    || !attached.files.is_empty() =>
            {
                Some(
                    parcel(rows, lineage.agent.agent, attached)
                        .await?,
                )
            }
            _ => None,
        };
        let review = match &lineage {
            Some(lineage)
                if matches!(
                    lineage.agent.status,
                    Status::Hiring | Status::Live
                ) =>
            {
                let (images, files) = parcel
                    .as_ref()
                    .map(|(_, images, files)| {
                        (images.clone(), files.clone())
                    })
                    .unwrap_or_default();
                let line = line(say, task, &chief, images, files);
                reviews::intercept(rows, lineage, task, &line).await?
            }
            _ => None,
        };
        Ok(Target {
            lineage,
            task,
            chief,
            parcel,
            review,
        })
    }

    fn decide(
        &self,
        _by: &Client,
        target: Target,
    ) -> Decision<Said, Self::Output> {
        let say = &self.say;
        let Some(lineage) = target.lineage else {
            return Decision::Answer(SayAnswer::refused(
                SayRefusal::NoSuchAgent {
                    address: say.0.to.0.to_string(),
                },
            ));
        };
        let agent = &lineage.agent;
        match &agent.status {
            Status::Ended(left) => Decision::Answer(
                SayAnswer::refused(SayRefusal::Left {
                    agent: agent.name.clone(),
                    left: left.clone(),
                }),
            ),
            Status::Hiring | Status::Live => {
                if let Err(why) = &self.attached {
                    return Decision::Answer(SayAnswer::refused(
                        why.clone(),
                    ));
                }
                let (opened, images, files) = match target.parcel {
                    Some((opened, images, files)) => {
                        (Some(opened), images, files)
                    }
                    None => (None, Vec::new(), Vec::new()),
                };
                let said = match target.review {
                    Some(_) => Said::Review(target.task),
                    None => Said::Line {
                        task: target.task,
                        chief: target.chief,
                        images,
                        files,
                    },
                };
                let mut decided =
                    Decided::new(said).change(tasks::Change::Line {
                        task: target.task,
                        agent: agent.agent,
                        payer: chiefs::payer_of(&lineage),
                        text: say.0.text.clone(),
                    });
                if agent.can_take_purpose()
                    && let Some(purpose) =
                        tree::purpose_from(agent.agent, &say.0.text)
                {
                    decided = decided.change(purpose);
                }
                decided.changes.extend(opened);
                decided
                    .changes
                    .extend(target.review.into_iter().flatten());
                Decision::Change(decided)
            }
        }
    }

    fn deliveries(&self, _by: &Client, said: &Said) -> Calls {
        match (to(&self.say), said) {
            (
                Some(to),
                Said::Line {
                    task,
                    chief,
                    images,
                    files,
                },
            ) => Calls::after_answer(Delivery::Line {
                to,
                submit: line(
                    &self.say,
                    *task,
                    chief,
                    images.clone(),
                    files.clone(),
                ),
            }),
            _ => Calls::none(),
        }
    }

    /// The realm keeps the line from here: it is given after the
    /// answer, or after its review.
    fn answer(&self, said: Said, _made: &Made) -> Self::Output {
        let (Said::Line { task, .. } | Said::Review(task)) = said;
        SayAnswer::Accepted {
            task: task.on_the_wire(),
        }
    }
}

fn to(say: &Say) -> Option<AgentNumber> {
    u32::try_from(say.0.to.0).ok().map(AgentNumber)
}

/// The parcel of a line's `attached` to `receiver`, and what the
/// agent is told of each file: the images first, then the files.
async fn parcel(
    rows: &Rows<'_>,
    receiver: AgentNumber,
    attached: &Attached,
) -> Result<(change::Change, Vec<LineImage>, Vec<LineFile>)> {
    let parcel =
        ParcelId(u32::try_from(rows.next_id("parcel").await?)?);
    let told =
        |file: usize, entry: &ParcelEntry| -> Result<ParcelFile> {
            Ok(ParcelFile {
                parcel,
                file: u32::try_from(file)?,
                path: entry.path.clone(),
                size: entry.size,
            })
        };
    let images = attached
        .images
        .iter()
        .enumerate()
        .map(|(file, (image, entry))| {
            Ok(LineImage {
                image: image.clone(),
                file: told(file, entry)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let files = attached
        .files
        .iter()
        .enumerate()
        .map(|(file, (attached, entry))| {
            Ok(LineFile {
                attached: attached.clone(),
                file: told(images.len() + file, entry)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let opened = parcels::Change::Line {
        parcel,
        receiver,
        files: attached
            .images
            .iter()
            .map(|(_, entry)| entry)
            .chain(attached.files.iter().map(|(_, entry)| entry))
            .cloned()
            .collect(),
    };
    Ok((opened.into(), images, files))
}

fn line(
    say: &Say,
    task: TaskId,
    chief: &str,
    images: Vec<LineImage>,
    files: Vec<LineFile>,
) -> Submit {
    Submit {
        id: InputId(say.0.id.as_str().to_owned()),
        task: Some(task.on_the_wire()),
        input: Input::Line {
            text: say.0.text.clone(),
            images,
            files,
            note: None,
            chief: Some(chief.to_owned()),
        },
    }
}

#[cfg(test)]
mod tests {
    use hamlet_actor::Action as _;
    use hamlet_messages::agent::AgentKind;
    use hamlet_messages::realm::budget::Payer;
    use hamlet_messages::realm::{HamletId, HowEnded, Left, Tier};
    use hamlet_messages::sender::Hand;
    use hamlet_messages::workspace::RealmTask;
    use hamlet_messages::workspace::reach::Mode;
    use hamlet_protocol::client::{
        self as protocol, AgentId, RequestId,
    };

    use super::*;
    use crate::realm::chiefs::limits;
    use crate::realm::testing::Started;
    use crate::realm::tree::Node;
    use crate::realm::{act, marker, record};

    fn task_7() -> TaskId {
        TaskId::of(&RealmTask("7".to_owned())).unwrap()
    }

    fn say(id: &RequestId, to: u64, text: &str) -> Say {
        Say(protocol::Say {
            files: Vec::new(),
            id: id.clone(),
            images: Vec::new(),
            text: text.to_owned(),
            to: AgentId(to),
        })
    }

    fn line_said() -> Said {
        Said::Line {
            task: task_7(),
            chief: "chief".to_owned(),
            images: Vec::new(),
            files: Vec::new(),
        }
    }

    fn hamlet() -> HamletId {
        HamletId("h".to_owned())
    }

    fn lineage(agent: Node) -> Lineage {
        let hamlet =
            (agent.kind == AgentKind::Hire).then(|| chiefs::Hamlet {
                id: hamlet(),
                mayor: AgentNumber(2),
                tier: Tier::Fifty,
                limits: limits::Limits::default(),
                folder: None,
                mode: Mode::Manual,
            });
        Lineage {
            agent,
            leads: Vec::new(),
            hires: Vec::new(),
            hamlet,
        }
    }

    fn mayor(status: Status, has_purpose: bool) -> Lineage {
        lineage(Node {
            agent: AgentNumber(2),
            name: record::bare(AgentNumber(2)),
            kind: AgentKind::Hire,
            status,
            lead: None,
            has_purpose,
        })
    }

    fn hand() -> Lineage {
        lineage(Node {
            agent: Hand::NUMBER,
            name: record::bare(Hand::NUMBER),
            kind: AgentKind::Hand,
            status: Status::Live,
            lead: None,
            has_purpose: false,
        })
    }

    fn decide(
        lineage: Option<Lineage>,
        text: &str,
    ) -> Decision<Said, SayAnswer> {
        let to = lineage
            .as_ref()
            .map_or(9, |lineage| u64::from(lineage.agent.agent.0));
        let target = Target {
            lineage,
            task: task_7(),
            chief: "chief".to_owned(),
            parcel: None,
            review: None,
        };
        let lined = Lined {
            say: say(&RequestId::new(), to, text),
            attached: Ok(Attached::default()),
        };
        lined.decide(&Client, target)
    }

    fn opened(
        agent: AgentNumber,
        payer: Payer,
        text: &str,
    ) -> tasks::Change {
        tasks::Change::Line {
            task: task_7(),
            agent,
            payer,
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_hire_with_no_purpose_takes_the_lines_first_line() {
        let by_hamlet = Payer::Hamlet { hamlet: hamlet() };
        let text = "\n   build the mill  \nthen the barn";

        assert_eq!(
            decide(Some(mayor(Status::Live, false)), text),
            Decision::Change(
                Decided::new(line_said())
                    .change(opened(
                        AgentNumber(2),
                        by_hamlet.clone(),
                        text,
                    ))
                    .change(tree::Change::Purpose {
                        hire: AgentNumber(2),
                        text: "build the mill".to_owned(),
                    })
            )
        );
        let only_the_task = |agent, payer, text| {
            Decision::Change(
                Decided::new(line_said())
                    .change(opened(agent, payer, text)),
            )
        };
        let blank = " \n\t\n";
        assert_eq!(
            decide(Some(mayor(Status::Live, true)), text),
            only_the_task(AgentNumber(2), by_hamlet.clone(), text)
        );
        assert_eq!(
            decide(Some(mayor(Status::Live, false)), blank),
            only_the_task(AgentNumber(2), by_hamlet, blank)
        );
        assert_eq!(
            decide(Some(hand()), text),
            only_the_task(Hand::NUMBER, Payer::Realm, text)
        );
    }

    #[test]
    fn a_line_to_a_hire_that_left_is_refused_with_how_and_why() {
        let left = Left {
            how: HowEnded::ByTheChief,
            why: Some("done".to_owned()),
        };

        assert_eq!(
            decide(
                Some(mayor(Status::Ended(left.clone()), false)),
                "hi"
            ),
            Decision::Answer(SayAnswer::refused(SayRefusal::Left {
                agent: record::bare(AgentNumber(2)),
                left,
            }))
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_line_the_agent_does_not_take_keeps_its_one_task() {
        // the started realm runs no agent, so every Submit fails: the
        // line is accepted, and a repeat answers the same task.
        let started = Started::new().await;
        started.found(&hamlet(), AgentNumber(2)).await;
        started
            .commit(tree::Change::Live {
                agent: AgentNumber(2),
            })
            .await
            .unwrap();
        let run = async |say: Say| {
            let id = marker::Id::Request(say.0.id.clone());
            let key = marker::Key::of(&Client, id, Say::NAME, &say)
                .unwrap();
            let lined =
                Lined::of(&started.realm, &started.ctx, &key, say)
                    .await
                    .unwrap();
            act::run_keyed(
                &started.realm,
                &started.ctx,
                Client,
                Some(key),
                Say::NAME,
                lined,
            )
            .await
            .unwrap()
        };
        let id = RequestId::new();

        let first = run(say(&id, 2, "build the mill")).await;
        let again = run(say(&id, 2, "build the mill")).await;
        let beyond =
            run(say(&RequestId::new(), u64::MAX, "hi")).await;
        let tasks = started
            .rows()
            .value::<u32>(
                "SELECT COUNT(*) FROM task WHERE origin = 'chief'",
                Vec::new(),
            )
            .await
            .unwrap();

        assert!(
            matches!(first, SayAnswer::Accepted { .. }),
            "the realm accepts the line: {first:?}"
        );
        assert_eq!(again, first);
        assert_eq!(
            beyond,
            SayAnswer::refused(SayRefusal::NoSuchAgent {
                address: u64::MAX.to_string()
            })
        );
        assert_eq!(tasks, Some(1));
        started.stop().await;
    }

    #[test]
    fn a_place_moves_with_the_scrubbed_words_before_it() {
        let image = |at| AttachedImage {
            id: "ab".repeat(32).parse().unwrap(),
            at,
        };
        let mut said = say(&RequestId::new(), 2, "key sk-1 here é");
        said.0.images = vec![image(Some(8)), image(None)];
        let scrub = |text: &str| text.replace("sk-1", "[REDACTED]");

        assert_eq!(placed(&mut said.0, scrub), Ok(()));
        assert_eq!(said.0.text, "key [REDACTED] here é");
        assert_eq!(said.0.images, vec![image(Some(14)), image(None)]);
        let mut inside = say(&RequestId::new(), 2, "é");
        inside.0.images = vec![image(Some(1))];
        assert_eq!(
            placed(&mut inside.0, scrub),
            Err(SayRefusal::NotAPlace { at: 1, length: 2 })
        );
    }
}
