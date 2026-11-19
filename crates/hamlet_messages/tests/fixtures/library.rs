//! The fixtures of the library's messages, and of the calls it makes
//! on an agent.

use hamlet_messages::MOST_PART;
use hamlet_messages::address::{Library, Party};
use hamlet_messages::bytes::{Base64, Sha256};
use hamlet_messages::library::{
    BadOffer, BadSkill, DropCopy, DropOfferParts, NoOffer, NoSkill,
    Offer, OfferAnswer, OfferId, OfferRead, OfferRefusal,
    OnTheShelves, ReadOffer, ReadShelves, ShelfList, Shelves,
    ShelvesChanged, Skill, Unshelve, Verdict, VerdictRefusal,
};
use hamlet_messages::profile::{By, Piece, TooLarge};
use hamlet_messages::realm::{Done, Refusable};
use hamlet_messages::workspace::AgentNumber;

use super::cases::{Cases, Union, family, tagged, union};
use super::profile::request;
use super::{Fixtures, call};

pub fn enums() -> Vec<Cases> {
    vec![
        tagged::<OfferAnswer>(),
        tagged::<OfferRead>(),
        tagged::<NoOffer>(),
        tagged::<BadOffer>(),
        tagged::<BadSkill>(),
        tagged::<OnTheShelves>(),
        tagged::<VerdictRefusal>(),
        tagged::<NoSkill>(),
    ]
}

pub fn unions() -> Vec<Union> {
    vec![union!(OfferRefusal {
        Shape: BadOffer,
        Size: TooLarge,
        Skill: BadSkill,
        Shelves: OnTheShelves,
    })]
}

fn shelves() -> ShelfList {
    ShelfList {
        version: 5,
        skills: vec![Skill {
            name: "fixtures".into(),
            description: "Writes the fixtures of a message".into(),
            usage: "Read skills/fixtures/SKILL.md".into(),
        }],
    }
}

fn skill() -> String {
    "fixtures".into()
}

pub fn the_librarys_messages(fixtures: &mut Fixtures) {
    fixtures.answer("library/address", &Library.address().unwrap());
    fixtures.args("library/shelves", &Shelves { version_seen: None });
    fixtures.args(
        "library/shelves.seen",
        &Shelves {
            version_seen: Some(4),
        },
    );
    fixtures.answer("library/shelves", &shelves());
    fixtures.args("library/read_shelves", &ReadShelves);
    fixtures.answer("library/read_shelves", &shelves());
    let offer = |part, last| Offer {
        call: call("call-7"),
        skill: skill(),
        reason: "Every hire writes fixtures".into(),
        part,
        last,
        files: vec![Piece {
            path: "SKILL.md".into(),
            offset: 0,
            bytes: Base64(b"---\nname: fixtures\n---\n".to_vec()),
            last: true,
            executable: false,
        }],
    };
    fixtures.args("library/offer", &offer(0, false));
    fixtures.args("library/offer.last", &offer(1, true));
    for (arm, answer) in [
        ("kept", OfferAnswer::Kept { offer: OfferId(9) }),
        ("more", OfferAnswer::More),
        (
            "refused.not_a_skill_name",
            OfferAnswer::refused(
                BadOffer::NotASkillName {
                    skill: "../fixtures".into(),
                }
                .into(),
            ),
        ),
        (
            "refused.not_a_path",
            OfferAnswer::refused(
                BadOffer::NotAPath {
                    path: "a\\b.py".into(),
                }
                .into(),
            ),
        ),
        (
            "refused.part_too_large",
            OfferAnswer::refused(OfferRefusal::from(
                TooLarge::PartTooLarge {
                    most: MOST_PART as u32,
                },
            )),
        ),
        (
            "refused.skill_too_large",
            OfferAnswer::refused(
                BadSkill::SkillTooLarge { skill: skill() }.into(),
            ),
        ),
        (
            "refused.no_front_matter",
            OfferAnswer::refused(
                BadSkill::NoFrontMatter { skill: skill() }.into(),
            ),
        ),
        (
            "refused.already_shelved",
            OfferAnswer::refused(
                OnTheShelves::AlreadyShelved { skill: skill() }
                    .into(),
            ),
        ),
        (
            "refused.being_judged",
            OfferAnswer::refused(
                OnTheShelves::BeingJudged { skill: skill() }.into(),
            ),
        ),
    ] {
        fixtures.kept(&format!("library/offer.{arm}"), &answer);
    }
    fixtures
        .args("library/read_offer", &ReadOffer { offer: OfferId(9) });
    for (arm, answer) in [
        (
            "offered",
            OfferRead::Offered {
                agent: AgentNumber(7),
                skill: skill(),
                reason: "Every hire writes fixtures".into(),
                paths: vec!["SKILL.md".into(), "write.py".into()],
                skill_md: "---\nname: fixtures\n---\n".into(),
            },
        ),
        (
            "refused.no_such_offer",
            OfferRead::refused(NoOffer::NoSuchOffer {
                offer: OfferId(9),
            }),
        ),
    ] {
        fixtures
            .answer(&format!("library/read_offer.{arm}"), &answer);
    }
    fixtures.args(
        "library/verdict",
        &Verdict {
            id: request(),
            offer: OfferId(9),
            accepted: true,
            words: "Shelved: it is clear and general".into(),
        },
    );
    for (arm, answer) in [
        ("done", Done::Done),
        (
            "refused.no_such_offer",
            Done::refused(VerdictRefusal::NoSuchOffer {
                offer: OfferId(9),
            }),
        ),
        (
            "refused.judged",
            Done::refused(VerdictRefusal::Judged {
                offer: OfferId(9),
            }),
        ),
    ] {
        fixtures.kept(&format!("library/verdict.{arm}"), &answer);
    }
    fixtures.args(
        "library/unshelve",
        &Unshelve {
            id: request(),
            skill: skill(),
            reason: Some("It is out of date".into()),
            by: By::Agent {
                agent: AgentNumber(1),
            },
        },
    );
    for (arm, answer) in [
        ("done", Done::Done),
        (
            "refused.no_such_skill",
            Done::refused(NoSkill::NoSuchSkill { skill: skill() }),
        ),
    ] {
        fixtures.kept(&format!("library/unshelve.{arm}"), &answer);
    }
    fixtures.args(
        "library/drop_offer_parts",
        &DropOfferParts {
            agent: AgentNumber(7),
        },
    );
    fixtures.answer("library/drop_offer_parts", &());
    fixtures.args(
        "library/shelves_changed",
        &ShelvesChanged { list: shelves() },
    );
    fixtures.answer("library/shelves_changed", &());
    fixtures.args(
        "library/drop_copy",
        &DropCopy {
            skill: skill(),
            digest: Sha256([0xab; 32]),
        },
    );
    fixtures.answer("library/drop_copy", &());
}
