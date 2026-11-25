//! The fixtures of the acts across actors: a merge of two hires, a
//! hire's return and erasure, a hamlet's removal, return and erasure,
//! and a profile's fork, archive, return, erasure and place in the
//! city.

use hamlet_messages::profile::{Missing, VersionName};
use hamlet_messages::realm::hamlets::{
    EraseHamlet, RemoveHamlet, RestoreHamlet,
};
use hamlet_messages::realm::hires::{
    EndedRefusal, EraseHire, Merge, MergeRefusal, MergedTo, Restore,
    Restored,
};
use hamlet_messages::realm::profiles::{
    ArchiveProfile, CreateRefusal, EraseProfile, ForkProfile,
    ForkRefusal, Forked, LifeRefusal, Lifecycle, LifecycleRefusal,
    MoveHouse, ProfileRefusal, RestoreProfile, TurnAway, Welcome,
};
use hamlet_messages::realm::{
    AgentRefusal, HamletRefusal, Named, ProfileId,
};

use super::super::Fixtures;
use super::super::cases::{Cases, Union, family, tagged, union};
use super::{
    HIRE, MAYOR, agent_refusal, answers, asked, by_a_lead, callers,
    done, hamlet, hire, lead_refusal, shown, tool,
};

const PROFILE: ProfileId = ProfileId(4);

pub fn enums() -> Vec<Cases> {
    vec![
        tagged::<MergeRefusal>(),
        tagged::<EndedRefusal>(),
        tagged::<Restored>(),
        tagged::<LifeRefusal>(),
        tagged::<Forked>(),
    ]
}

pub fn unions() -> Vec<Union> {
    vec![
        lead_refusal::<MergeRefusal>(),
        agent_refusal::<HamletRefusal>(),
        union!(ForkRefusal {
            Profile: ProfileRefusal,
            Missing: Missing,
            Create: CreateRefusal,
        }),
        union!(LifecycleRefusal {
            Profile: ProfileRefusal,
            Life: LifeRefusal,
        }),
    ]
}

pub fn the_acts_across_actors(fixtures: &mut Fixtures) {
    the_merges_and_the_ended_hires(fixtures);
    the_hamlets_removed(fixtures);
    the_profiles_lives(fixtures);
}

fn kept() -> Named {
    Named::Agent { agent: MAYOR }
}

fn the_merges_and_the_ended_hires(fixtures: &mut Fixtures) {
    let merge = || Merge {
        absorbed: hire(),
        into: kept(),
        why: Some("Their work overlaps".into()),
    };
    fixtures.args("realm/chief_merge", &asked(merge()));
    fixtures.args(
        "realm/chief_merge.no_reason",
        &asked(Merge {
            why: None,
            ..merge()
        }),
    );
    fixtures.args("realm/merge_hires", &tool(merge()));
    let merge_refusals = || {
        vec![
            (
                "no_such_agent",
                MergeRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            (
                "not_a_hire",
                MergeRefusal::NotAHire {
                    name: "hand".into(),
                },
            ),
            ("same", MergeRefusal::Same),
            (
                "other_hamlet",
                MergeRefusal::OtherHamlet { name: shown(HIRE) },
            ),
            ("the_mayor", MergeRefusal::TheMayor),
        ]
    };
    done(fixtures, "realm/chief_merge", merge_refusals());
    answers(
        fixtures,
        "realm/merge_hires",
        [(
            "done",
            MergedTo::Done {
                absorbed: shown(HIRE),
                into: shown(MAYOR),
            },
        )],
        by_a_lead(merge_refusals()),
    );
    fixtures.args(
        "realm/chief_restore",
        &asked(Restore {
            hire: hire(),
            why: Some("We need it again".into()),
        }),
    );
    let ended_refusals = || {
        vec![
            (
                "no_such_agent",
                EndedRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            (
                "not_a_hire",
                EndedRefusal::NotAHire {
                    name: "hand".into(),
                },
            ),
            ("not_left", EndedRefusal::NotLeft { hire: shown(HIRE) }),
            (
                "hamlet_removed",
                EndedRefusal::HamletRemoved { hamlet: hamlet() },
            ),
            (
                "profile_gone",
                EndedRefusal::ProfileGone {
                    label: "scholar#4 (Ada)".into(),
                },
            ),
            (
                "full",
                EndedRefusal::Full {
                    hamlet: hamlet(),
                    most: 50,
                },
            ),
        ]
    };
    answers(
        fixtures,
        "realm/chief_restore",
        [("back", Restored::Back), ("split", Restored::Split)],
        ended_refusals(),
    );
    fixtures.args(
        "realm/chief_erase",
        &asked(EraseHire { hire: hire() }),
    );
    done(fixtures, "realm/chief_erase", ended_refusals());
}

fn the_hamlets_removed(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/chief_remove_hamlet",
        &asked(RemoveHamlet {
            hamlet: hamlet(),
            why: Some("closed by the chief".into()),
        }),
    );
    fixtures.args(
        "realm/remove_hamlet",
        &tool(RemoveHamlet {
            hamlet: hamlet(),
            why: None,
        }),
    );
    fixtures.args(
        "realm/chief_restore_hamlet",
        &asked(RestoreHamlet {
            hamlet: hamlet(),
            why: None,
        }),
    );
    fixtures.args(
        "realm/chief_erase_hamlet",
        &asked(EraseHamlet { hamlet: hamlet() }),
    );
    let hamlet_refusals = || {
        vec![
            (
                "no_such_hamlet",
                HamletRefusal::NoSuchHamlet { id: hamlet() },
            ),
            ("erased", HamletRefusal::Erased { id: hamlet() }),
            ("removed", HamletRefusal::Removed { id: hamlet() }),
            ("living", HamletRefusal::Living { id: hamlet() }),
            (
                "profile_gone",
                HamletRefusal::ProfileGone {
                    label: "scholar#4 (Ada)".into(),
                },
            ),
        ]
    };
    for name in [
        "realm/chief_remove_hamlet",
        "realm/chief_restore_hamlet",
        "realm/chief_erase_hamlet",
    ] {
        done(fixtures, name, hamlet_refusals());
    }
    let others = [
        (
            "no_such_folder",
            HamletRefusal::NoSuchFolder {
                path: "missing".into(),
            },
        ),
        (
            "bad_name",
            HamletRefusal::BadName {
                name: String::new(),
            },
        ),
        (
            "bad_site",
            HamletRefusal::BadSite {
                site: "NaN, 0".into(),
            },
        ),
        ("misshapen_limits", HamletRefusal::MisshapenLimits),
    ];
    done(
        fixtures,
        "realm/remove_hamlet",
        callers::<AgentRefusal<HamletRefusal>>()
            .into_iter()
            .chain(
                hamlet_refusals().into_iter().chain(others).map(
                    |(case, why)| (case, AgentRefusal::Act(why)),
                ),
            )
            .collect(),
    );
}

fn the_profiles_lives(fixtures: &mut Fixtures) {
    let label = || "scholar#4 (Ada)".to_owned();
    fixtures.args(
        "realm/chief_fork_profile",
        &asked(ForkProfile {
            from: PROFILE,
            at: Some(VersionName("v1".into())),
            name: "Ada's sister".into(),
        }),
    );
    fixtures.args(
        "realm/chief_fork_profile.at_main",
        &asked(ForkProfile {
            from: PROFILE,
            at: None,
            name: "Ada's sister".into(),
        }),
    );
    answers(
        fixtures,
        "realm/chief_fork_profile",
        [(
            "forked",
            Forked::Forked {
                profile: ProfileId(5),
                from: label(),
            },
        )],
        vec![
            (
                "no_such_profile",
                ProfileRefusal::NoSuchProfile { profile: PROFILE }
                    .into(),
            ),
            (
                "apart",
                ProfileRefusal::Apart { label: label() }.into(),
            ),
            (
                "no_version",
                Missing::NoVersion {
                    profile: PROFILE,
                    version: VersionName("v9".into()),
                }
                .into(),
            ),
            (
                "bad_name",
                CreateRefusal::BadName {
                    name: String::new(),
                }
                .into(),
            ),
            ("long_character", CreateRefusal::LongCharacter.into()),
        ],
    );
    fixtures.args(
        "realm/chief_archive_profile",
        &asked(ArchiveProfile {
            profile: PROFILE,
            why: Some("Not needed".into()),
        }),
    );
    fixtures.args(
        "realm/chief_restore_profile",
        &asked(RestoreProfile {
            profile: PROFILE,
            why: None,
        }),
    );
    fixtures.args(
        "realm/chief_erase_profile",
        &asked(EraseProfile { profile: PROFILE }),
    );
    fixtures.args(
        "realm/chief_welcome",
        &asked(Welcome {
            profile: PROFILE,
            why: None,
        }),
    );
    fixtures.args(
        "realm/chief_turn_away",
        &asked(TurnAway {
            profile: PROFILE,
            why: Some("Not yet".into()),
        }),
    );
    fixtures.args(
        "realm/chief_move_house",
        &asked(MoveHouse {
            profile: PROFILE,
            x: 12.5,
            z: -4.0,
            degrees: Some(90.0),
        }),
    );
    let life_refusals =
        || -> Vec<(&'static str, LifecycleRefusal)> {
            let life = [
                ("gone", LifeRefusal::Gone { label: label() }),
                ("erased", LifeRefusal::Erased { label: label() }),
                ("living", LifeRefusal::Living { label: label() }),
                (
                    "at_harbor",
                    LifeRefusal::AtHarbor { label: label() },
                ),
                ("housed", LifeRefusal::Housed { label: label() }),
                (
                    "turned_away",
                    LifeRefusal::TurnedAway { label: label() },
                ),
                (
                    "bad_site",
                    LifeRefusal::BadSite {
                        site: "NaN, 0".into(),
                    },
                ),
                ("crowded", LifeRefusal::Crowded { label: label() }),
                ("the_mayor", LifeRefusal::TheMayor),
                (
                    "full",
                    LifeRefusal::Full {
                        hamlet: hamlet(),
                        most: 50,
                    },
                ),
            ];
            [
                (
                    "no_such_profile",
                    ProfileRefusal::NoSuchProfile {
                        profile: PROFILE,
                    }
                    .into(),
                ),
                (
                    "apart",
                    ProfileRefusal::Apart { label: label() }.into(),
                ),
            ]
            .into_iter()
            .chain(
                life.into_iter()
                    .map(|(case, why)| (case, why.into())),
            )
            .collect()
        };
    for name in [
        "realm/chief_archive_profile",
        "realm/chief_restore_profile",
        "realm/chief_erase_profile",
        "realm/chief_welcome",
        "realm/chief_turn_away",
        "realm/chief_move_house",
    ] {
        answers(
            fixtures,
            name,
            [("done", Lifecycle::Done { label: label() })],
            life_refusals(),
        );
    }
}
