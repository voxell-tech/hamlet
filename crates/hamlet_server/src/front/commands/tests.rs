use std::sync::Arc;

use hamlet_messages::profile::{Missing, VersionName};
use hamlet_messages::realm::hires::{EndedRefusal, MergeRefusal};
use hamlet_messages::realm::profiles::{
    CreateRefusal, ForkRefusal, LifeRefusal, LifecycleRefusal,
    ProfileRefusal,
};
use hamlet_messages::realm::world::{
    AgentSeen, ReachSeen, Seen, Starter, Status,
};
use hamlet_messages::realm::{
    AgentName, HamletId, HamletRefusal, HowEnded, Left, ProfileId,
};
use hamlet_protocol::client::Confinement;

use super::*;
use crate::front::view::Own;

const DOCS: &str = "docs";

const ADA: &str = "scholar#2 (Ada)";

/// The Hand, the living hires 2 and 3, the worker 4 and the ended
/// hire 5.
fn view() -> View {
    let seen = |agent: u32, status: Status, worker: bool| AgentSeen {
        agent: AgentNumber(agent),
        hamlet: Some(HamletId(DOCS.into())),
        lead: None,
        status,
        reach: ReachSeen::None,
        worker: worker.then_some(Starter::Chief),
        nickname: None,
        runs: None,
        model: None,
        effort: None,
        fast: None,
        allowance_micros: None,
    };
    let ended = Status::Ended {
        left: Left {
            how: HowEnded::ByTheChief,
            why: None,
        },
    };
    View {
        own: Arc::new(Own {
            chief: "Ada".into(),
            confinement: Confinement::Insecure,
            realm: "/realm".into(),
            epoch: 0,
        }),
        world: Seen {
            agents: vec![
                seen(1, Status::Live, false),
                seen(2, Status::Live, false),
                seen(3, Status::Live, false),
                seen(4, Status::Live, true),
                seen(5, ended, false),
            ],
            ..Seen::default()
        },
    }
}

fn at(text: &str) -> Target {
    parse::target(text, None).unwrap()
}

fn read(reply: Result<impl Display, Refusal>) -> String {
    reply.map_or_else(
        |refusal| refusal.to_string(),
        |said| said.to_string(),
    )
}

fn hamlet() -> HamletId {
    HamletId(DOCS.into())
}

fn hire_name(agent: u32) -> AgentName {
    AgentName {
        agent: AgentNumber(agent),
        name: format!("hire#{agent}"),
    }
}

/// Asserts that `reply` gives each answer the words beside it.
fn reads<A>(
    reply: impl Fn(A) -> Result<String, Refusal>,
    cases: impl IntoIterator<Item = (A, &'static str)>,
) {
    for (answer, words) in cases {
        assert_eq!(read(reply(answer)), words);
    }
}

fn refused_with<W>(why: impl Into<W>) -> Done<W> {
    Done::Refused { why: why.into() }
}

#[test]
fn a_line_told_on_has_one_id_for_each_command() {
    let id = RequestId::new();
    assert_eq!(line_id(&id), line_id(&id));
    assert_ne!(line_id(&id), id);
}

#[test]
fn the_speakers_oldest_wait_comes_before_an_older_one_of_another() {
    let (one, two) = (AgentNumber(1), AgentNumber(2));
    let open = [(5, two), (3, one), (7, two)];
    assert_eq!(oldest_wait(open, two), Some(5));
    assert_eq!(oldest_wait(open, AgentNumber(9)), Some(3));
    assert_eq!(oldest_wait([], one), None);
}

#[test]
fn merge_is_worded_as_today() {
    let view = view();
    let ends = |absorbed: &str, into: &str| {
        read(merge_ends(&view, &at(absorbed), &at(into)).map(
            |(absorbed, into)| format!("{} {}", absorbed.0, into.0),
        ))
    };
    assert_eq!(
        ends("worker#4", "hire#9"),
        "no agent has the address `hire#9`"
    );
    assert_eq!(
        ends("hire#5", "hire#2"),
        "no agent has the address `hire#5`"
    );
    assert_eq!(ends("worker#4", "hand"), "`worker#4` is not a hire");
    assert_eq!(ends("hire#3", "hand"), "`hand` is not a hire");
    assert_eq!(ends("hire#3", "hire#2"), "3 2");
    let merged = || words::merged("hire#3", "mayor");
    reads(
        |answer: Done<MergeRefusal>| done(answer, merged),
        [
            (Done::Done, "Merged `hire#3` into `mayor`."),
            (
                refused_with(MergeRefusal::Same),
                "a road needs two different hires",
            ),
            (
                refused_with(MergeRefusal::OtherHamlet {
                    name: hire_name(3),
                }),
                "`hire#3` is in another hamlet",
            ),
            (
                refused_with(MergeRefusal::TheMayor),
                "a mayor cannot be ended; seat another mayor first",
            ),
        ],
    );
}

#[test]
fn restore_is_worded_as_today() {
    let view = view();
    let hire = |text: &str| {
        read(hire_of(&view, &at(text)).map(|agent| agent.0))
    };
    assert_eq!(hire("hire#9"), "no agent has the address `hire#9`");
    assert_eq!(hire("worker#4"), "`worker#4` is not a hire");
    assert_eq!(hire("hire#5"), "5");
    reads(
        |answer| restored(answer, "hire#5"),
        [
            (Restored::Back, "`hire#5` is back."),
            (
                Restored::Split,
                "`hire#5` is back; what it brought to the hire it \
                 was merged into is taken out again.",
            ),
            (
                Restored::Refused {
                    why: EndedRefusal::NotLeft { hire: hire_name(2) },
                },
                "`hire#2` has not left",
            ),
            (
                Restored::Refused {
                    why: EndedRefusal::Erased { hire: hire_name(5) },
                },
                "`hire#5` was erased",
            ),
            (
                Restored::Refused {
                    why: EndedRefusal::HamletRemoved {
                        hamlet: hamlet(),
                    },
                },
                "the hamlet `docs` was removed; restore it first",
            ),
            (
                Restored::Refused {
                    why: EndedRefusal::ProfileGone {
                        label: ADA.into(),
                    },
                },
                "the scholar `scholar#2 (Ada)` no longer lives in \
                 the city",
            ),
            (
                Restored::Refused {
                    why: EndedRefusal::Full {
                        hamlet: hamlet(),
                        most: 50,
                    },
                },
                "the hamlet `docs` holds its most hires, 50",
            ),
        ],
    );
}

#[test]
fn erase_is_worded_as_today() {
    let view = view();
    assert_eq!(
        read(hire_of(&view, &at("hand")).map(|agent| agent.0)),
        "`hand` is not a hire"
    );
    reads(
        |answer: Done<EndedRefusal>| {
            done(answer, || words::erased("hire#5"))
        },
        [
            (Done::Done, "`hire#5` was erased for good."),
            (
                refused_with(EndedRefusal::NotLeft {
                    hire: hire_name(2),
                }),
                "`hire#2` has not left",
            ),
            (
                refused_with(EndedRefusal::Erased {
                    hire: hire_name(5),
                }),
                "`hire#5` was erased",
            ),
        ],
    );
}

fn hamlet_refusals() -> [(Done<HamletRefusal>, &'static str); 3] {
    [
        (
            refused_with(HamletRefusal::NoSuchHamlet {
                id: hamlet(),
            }),
            "no hamlet has the id `docs`",
        ),
        (
            refused_with(HamletRefusal::Erased { id: hamlet() }),
            "the hamlet `docs` was erased",
        ),
        (
            refused_with(HamletRefusal::Removed { id: hamlet() }),
            "the hamlet `docs` was removed; restore it first",
        ),
    ]
}

fn living_hamlet() -> (Done<HamletRefusal>, &'static str) {
    (
        refused_with(HamletRefusal::Living { id: hamlet() }),
        "the hamlet `docs` is still living; remove it first",
    )
}

#[test]
fn remove_hamlet_is_worded_as_today() {
    let said = || words::hamlet_removed(DOCS);
    reads(
        |answer| done(answer, said),
        [(
            Done::Done,
            "The hamlet `docs` was removed and its hires ended.",
        )]
        .into_iter()
        .chain(hamlet_refusals()),
    );
}

#[test]
fn restore_hamlet_is_worded_as_today() {
    let [missing, erased, _] = hamlet_refusals();
    reads(
        |answer| done(answer, || words::hamlet_restored(DOCS)),
        [
            (Done::Done, "The hamlet `docs` is restored."),
            missing,
            erased,
            living_hamlet(),
            (
                refused_with(HamletRefusal::ProfileGone {
                    label: ADA.into(),
                }),
                "the scholar `scholar#2 (Ada)` no longer lives in \
                 the city",
            ),
        ],
    );
}

#[test]
fn erase_hamlet_is_worded_as_today() {
    let [missing, erased, _] = hamlet_refusals();
    reads(
        |answer| done(answer, || words::hamlet_erased(DOCS)),
        [
            (Done::Done, "The hamlet `docs` was erased for good."),
            missing,
            erased,
            living_hamlet(),
        ],
    );
}

#[test]
fn fork_is_worded_as_today() {
    let done = || Forked::Forked {
        profile: ProfileId(7),
        from: ADA.into(),
    };
    assert_eq!(
        read(forked(done(), None, "Notes")),
        "Forked scholar#2 (Ada) as Notes."
    );
    let v3 = VersionName("v3".into());
    assert_eq!(
        read(forked(done(), Some(&v3), "Notes")),
        "Forked scholar#2 (Ada) at v3 as Notes."
    );
    reads(
        |answer| forked(answer, Some(&v3), "Notes"),
        [
            (
                Forked::Refused {
                    why: ProfileRefusal::NoSuchProfile {
                        profile: ProfileId(2),
                    }
                    .into(),
                },
                "no scholar has the address `scholar#2`",
            ),
            (
                Forked::Refused {
                    why: Missing::NoVersion {
                        profile: ProfileId(2),
                        version: v3.clone(),
                    }
                    .into(),
                },
                "`scholar#2` has no version `v3`",
            ),
            (
                Forked::Refused {
                    why: ForkRefusal::from(CreateRefusal::BadName {
                        name: String::new(),
                    }),
                },
                "`` is not a name: use one line of 1 to 40 \
                 characters with no colon",
            ),
        ],
    );
}

fn life(why: LifeRefusal) -> Lifecycle {
    Lifecycle::Refused {
        why: LifecycleRefusal::from(why),
    }
}

fn no_profile() -> (Lifecycle, &'static str) {
    (
        Lifecycle::Refused {
            why: ProfileRefusal::NoSuchProfile {
                profile: ProfileId(2),
            }
            .into(),
        },
        "no scholar has the address `scholar#2`",
    )
}

fn gone() -> (Lifecycle, &'static str) {
    (
        life(LifeRefusal::Gone { label: ADA.into() }),
        "the scholar `scholar#2 (Ada)` no longer lives in the city",
    )
}

fn erased() -> (Lifecycle, &'static str) {
    (
        life(LifeRefusal::Erased { label: ADA.into() }),
        "the scholar `scholar#2 (Ada)` was erased",
    )
}

fn living() -> (Lifecycle, &'static str) {
    (
        life(LifeRefusal::Living { label: ADA.into() }),
        "the scholar `scholar#2 (Ada)` is still living; remove it \
         first",
    )
}

fn apart() -> (Lifecycle, &'static str) {
    (
        Lifecycle::Refused {
            why: ProfileRefusal::Apart { label: ADA.into() }.into(),
        },
        "`scholar#2 (Ada)` is a blank hire's own, not a scholar of \
         the city",
    )
}

fn done_with_ada() -> Lifecycle {
    Lifecycle::Done { label: ADA.into() }
}

#[test]
fn remove_profile_is_worded_as_today() {
    reads(
        |answer| lived(answer, words::profile_archived),
        [
            (
                done_with_ada(),
                "`scholar#2 (Ada)` left the city, with its hires.",
            ),
            no_profile(),
            gone(),
            (
                life(LifeRefusal::TheMayor),
                "a mayor cannot be ended; seat another mayor first",
            ),
        ],
    );
}

#[test]
fn restore_profile_is_worded_as_today() {
    reads(
        |answer| lived(answer, words::profile_restored),
        [
            (
                done_with_ada(),
                "`scholar#2 (Ada)` is back in the city.",
            ),
            no_profile(),
            erased(),
            living(),
            (
                life(LifeRefusal::Full {
                    hamlet: hamlet(),
                    most: 50,
                }),
                "the hamlet `docs` holds its most hires, 50",
            ),
        ],
    );
}

#[test]
fn erase_profile_is_worded_as_today() {
    reads(
        |answer| lived(answer, words::erased),
        [
            (
                done_with_ada(),
                "`scholar#2 (Ada)` was erased for good.",
            ),
            no_profile(),
            erased(),
            living(),
        ],
    );
}

#[test]
fn welcome_is_worded_as_today() {
    reads(
        |answer| lived(answer, words::welcomed),
        [
            (
                done_with_ada(),
                "`scholar#2 (Ada)` moved into its house in the city.",
            ),
            no_profile(),
            gone(),
            apart(),
            (
                life(LifeRefusal::Housed { label: ADA.into() }),
                "the scholar `scholar#2 (Ada)` lives in the city \
                 already",
            ),
        ],
    );
}

#[test]
fn turn_away_is_worded_as_today() {
    reads(
        |answer| lived(answer, words::turned_away),
        [
            (
                done_with_ada(),
                "`scholar#2 (Ada)` waits at the harbor.",
            ),
            no_profile(),
            gone(),
            apart(),
            (
                life(LifeRefusal::TurnedAway { label: ADA.into() }),
                "the scholar `scholar#2 (Ada)` was turned away \
                 already",
            ),
        ],
    );
}

#[test]
fn move_house_is_worded_as_today() {
    reads(
        |answer| {
            lived(answer, |label| {
                words::house_moved(label, 4.0, -5.5)
            })
        },
        [
            (
                done_with_ada(),
                "`scholar#2 (Ada)`'s house stands at 4, -5.5.",
            ),
            no_profile(),
            gone(),
            apart(),
            (
                life(LifeRefusal::AtHarbor { label: ADA.into() }),
                "the scholar `scholar#2 (Ada)` waits at the harbor; \
                 welcome it into the city first",
            ),
            (
                life(LifeRefusal::BadSite {
                    site: "inf, 5".into(),
                }),
                "`inf, 5` is not an address",
            ),
            (
                life(LifeRefusal::Crowded { label: ADA.into() }),
                "the house of `scholar#2 (Ada)` would stand on \
                 another house or the city's middle",
            ),
        ],
    );
}
