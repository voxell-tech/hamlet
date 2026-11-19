//! The fixtures of the acts that make, read and change a profile, and
//! of an agent's question to the librarian.

use hamlet_messages::profile::{
    BadCommit, BadPath, CommitId, Missing, Path, Protected,
    ReadAnswer, RevertAnswer, Revision, Sha1, TagAnswer, TagRefusal,
    TooLong, VersionName,
};
use hamlet_messages::realm::profiles::{
    AskLibrarian, CreateProfile, CreateRefusal, EditProfile,
    EditProfileRefusal, Made, ProfileRefusal, ReadProfile,
    ReadProfileRefusal, RevertProfile, RevertProfileRefusal,
    TagProfile, TagProfileRefusal,
};
use hamlet_messages::realm::{
    AgentRefusal, Gate, ProfileId, Standing,
};

use super::super::Fixtures;
use super::super::cases::{
    Cases, Family, Union, family, tagged, union,
};
use super::{answers, asked, by_an_agent, callers, done, tool};

const SCHOLAR: ProfileId = ProfileId(4);

pub fn enums() -> Vec<Cases> {
    vec![
        tagged::<ProfileRefusal>(),
        tagged::<CreateRefusal>(),
        tagged::<Made<CreateRefusal>>(),
    ]
}

pub fn unions() -> Vec<Union> {
    vec![
        union!(EditProfileRefusal {
            Profile: ProfileRefusal,
            Path: BadPath,
            Protected: Protected,
            Size: TooLong,
        }),
        union!(TagProfileRefusal {
            Profile: ProfileRefusal,
            Tag: TagRefusal,
        }),
        union!(RevertProfileRefusal {
            Profile: ProfileRefusal,
            Missing: Missing,
            Commit: BadCommit,
        }),
        super::agent_refusal::<CreateRefusal>(),
        by_an_agent_of::<TagProfileRefusal>(vec![
            family::<ProfileRefusal>(),
            family::<TagRefusal>(),
        ]),
        by_an_agent_of::<ReadProfileRefusal>(vec![
            family::<ProfileRefusal>(),
            family::<Missing>(),
            family::<BadCommit>(),
            family::<BadPath>(),
        ]),
        by_an_agent_of::<EditProfileRefusal>(vec![
            family::<ProfileRefusal>(),
            family::<BadPath>(),
            family::<Protected>(),
            family::<TooLong>(),
        ]),
        by_an_agent_of::<RevertProfileRefusal>(vec![
            family::<ProfileRefusal>(),
            family::<Missing>(),
            family::<BadCommit>(),
        ]),
    ]
}

/// `AgentRefusal<U>` for a union `U`, read as one union of the
/// caller's refusals and `U`'s families.
fn by_an_agent_of<U>(families: Vec<Family>) -> Union {
    let mut all = vec![family::<Standing>(), family::<Gate>()];
    all.extend(families);
    union::<AgentRefusal<U>>(all)
}

pub fn the_profiles_through_the_realm(fixtures: &mut Fixtures) {
    the_making_and_the_reading(fixtures);
    the_changes(fixtures);
    the_librarian(fixtures);
    the_hire_at_a_version(fixtures);
}

fn version() -> VersionName {
    VersionName("v1".into())
}

fn commit() -> CommitId {
    CommitId(Sha1([0xab; 20]))
}

fn no_version() -> Missing {
    Missing::NoVersion {
        profile: SCHOLAR,
        version: version(),
    }
}

fn no_commit() -> BadCommit {
    BadCommit::NoCommit {
        profile: SCHOLAR,
        commit: commit(),
    }
}

fn not_on_main() -> BadCommit {
    BadCommit::NotOnMain { commit: commit() }
}

fn not_a_path() -> BadPath {
    BadPath::NotAPath {
        path: "memory/gone.md".into(),
    }
}

pub(super) fn label() -> String {
    "scholar#4 (Ada)".into()
}

/// The refusals of an act on a profile, by the profile it names.
fn named<R: From<ProfileRefusal>>() -> Vec<(&'static str, R)> {
    vec![
        (
            "no_such_profile",
            ProfileRefusal::NoSuchProfile { profile: SCHOLAR }.into(),
        ),
        ("apart", ProfileRefusal::Apart { label: label() }.into()),
    ]
}

fn the_making_and_the_reading(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/create_profile",
        &tool(CreateProfile {
            name: "Ada".into(),
            character: Some("You write careful tests.".into()),
        }),
    );
    fixtures.args(
        "realm/create_profile.realms_character",
        &tool(CreateProfile {
            name: "Ada".into(),
            character: None,
        }),
    );
    answers(
        fixtures,
        "realm/create_profile",
        [(
            "made",
            Made::Made {
                profile: SCHOLAR,
                label: label(),
            },
        )],
        by_an_agent(vec![
            (
                "bad_name",
                CreateRefusal::BadName {
                    name: "a: b".into(),
                },
            ),
            ("long_character", CreateRefusal::LongCharacter),
        ]),
    );
    fixtures.args(
        "realm/read_profile",
        &tool(ReadProfile {
            profile: SCHOLAR,
            at: None,
            path: None,
        }),
    );
    fixtures.args(
        "realm/read_profile.file",
        &tool(ReadProfile {
            profile: SCHOLAR,
            at: Some(Revision::Version { version: version() }),
            path: Some("memory/tests.md".into()),
        }),
    );
    let mut refusals = named::<ReadProfileRefusal>();
    refusals.extend([
        ("no_version", no_version().into()),
        ("no_commit", no_commit().into()),
        ("not_on_main", not_on_main().into()),
        ("not_a_path", not_a_path().into()),
    ]);
    answers(
        fixtures,
        "realm/read_profile",
        [
            (
                "listed",
                ReadAnswer::Listed {
                    paths: vec![Path {
                        path: "memory/tests.md".into(),
                    }],
                },
            ),
            (
                "text",
                ReadAnswer::Text {
                    text: "# Tests\n".into(),
                    cut: false,
                },
            ),
        ],
        by_an_agent(refusals),
    );
}

fn the_changes(fixtures: &mut Fixtures) {
    let edit = EditProfile {
        profile: SCHOLAR,
        path: "CHARACTER.md".into(),
        text: "You write careful tests.".into(),
        message: Some("Sharper".into()),
    };
    fixtures.args("realm/edit_profile", &tool(edit.clone()));
    fixtures.args("realm/chief_edit_profile", &asked(edit));
    let mut edits = named::<EditProfileRefusal>();
    edits.extend([
        ("not_a_path", not_a_path().into()),
        (
            "withheld",
            Protected::Withheld {
                path: "inbox/letter.md".into(),
            }
            .into(),
        ),
        (
            "character_too_long",
            TooLong::CharacterTooLong { most: 4000 }.into(),
        ),
    ]);
    done(fixtures, "realm/chief_edit_profile", edits.clone());
    done(fixtures, "realm/edit_profile", by_an_agent(edits));

    let tag = TagProfile {
        profile: SCHOLAR,
        version: version(),
        note: "Before the refactor".into(),
    };
    fixtures.args("realm/tag", &tool(tag.clone()));
    fixtures.args("realm/chief_tag_profile", &asked(tag));
    let mut tags = named::<TagProfileRefusal>();
    tags.extend([
        (
            "bad_name",
            TagRefusal::BadName {
                version: VersionName("v 1".into()),
            }
            .into(),
        ),
        ("taken", TagRefusal::Taken { version: version() }.into()),
        (
            "no_such_commit",
            TagRefusal::NoSuchCommit {
                profile: SCHOLAR,
                commit: commit(),
            }
            .into(),
        ),
    ]);
    fn tagged<W>() -> TagAnswer<W> {
        TagAnswer::Tagged {
            version: version(),
            commit: commit(),
        }
    }
    answers(
        fixtures,
        "realm/chief_tag_profile",
        [("tagged", tagged())],
        tags.clone(),
    );
    answers(
        fixtures,
        "realm/tag",
        [("tagged", tagged())],
        by_an_agent(tags),
    );

    let revert = RevertProfile {
        profile: SCHOLAR,
        what: Revision::Commit { commit: commit() },
        reason: Some("It broke the tests".into()),
    };
    fixtures.args("realm/revert", &tool(revert.clone()));
    fixtures.args("realm/chief_revert_profile", &asked(revert));
    let mut reverts = named::<RevertProfileRefusal>();
    reverts.extend([
        ("no_version", no_version().into()),
        ("no_commit", no_commit().into()),
        ("not_on_main", not_on_main().into()),
    ]);
    fn reverted<W>() -> [(&'static str, RevertAnswer<W>); 2] {
        [
            (
                "reverted",
                RevertAnswer::Reverted {
                    commit: commit(),
                    clashes: vec![Path {
                        path: "memory/tests.md".into(),
                    }],
                },
            ),
            ("unchanged", RevertAnswer::Unchanged),
        ]
    }
    answers(
        fixtures,
        "realm/chief_revert_profile",
        reverted(),
        reverts.clone(),
    );
    answers(
        fixtures,
        "realm/revert",
        reverted(),
        by_an_agent(reverts),
    );
}

fn the_librarian(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/ask_librarian",
        &tool(AskLibrarian {
            question: "How do I lint the docs?".into(),
        }),
    );
    let standing: Vec<(&'static str, Standing)> =
        callers::<AgentRefusal<()>>()
            .into_iter()
            .filter_map(|(case, why)| match why {
                AgentRefusal::Standing(why) => Some((case, why)),
                _ => None,
            })
            .collect();
    done(fixtures, "realm/ask_librarian", standing);
}

fn the_hire_at_a_version(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/hire.at_a_version",
        &tool(hamlet_messages::realm::Hire {
            hamlet: None,
            under: None,
            profile: Some(SCHOLAR),
            version: Some(version()),
            purpose: Some("Check the links".into()),
            name: None,
        }),
    );
}
