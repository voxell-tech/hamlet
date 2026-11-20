//! The fixtures of a profile's messages, and of the calls it makes on
//! an agent.

use hamlet_messages::MOST_PART;
use hamlet_messages::bytes::Base64;
use hamlet_messages::profile::{
    BadCommit, BadPath, By, Character, CommitId, CommitSeen, Cursor,
    DreamEnd, DreamId, DreamOf, DreamOpen, DropSaveParts, Edit,
    EditRefusal, EndDream, Erase, FileChange, Files, FilesAnswer,
    Gone, Missing, Moved, NotAHire, Paper, PaperList, Papers, Path,
    Piece, Pin, PinAnswer, PinRefusal, PrepareDream, Prepared,
    ProfileInput, Protected, Purge, PurgeAnswer, PurgeRefusal, Read,
    ReadAnswer, ReadFileRefusal, Removal, RemovePath, Revert,
    RevertAnswer, RevertRefusal, Revision, Save, SaveAnswer,
    SaveRefusal, Sha1, Tag, TagAnswer, TagRefusal, TooLarge, TooLong,
    VersionList, VersionName, VersionSeen, Versions,
};
use hamlet_messages::realm::{Done, ProfileId, Refusable};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;

use super::cases::{Cases, Union, family, named, tagged, union};
use super::{AGENT, Fixtures, call};

pub fn enums() -> Vec<Cases> {
    vec![
        tagged::<ProfileInput>(),
        tagged::<Revision>(),
        tagged::<By>(),
        tagged::<DreamOf>(),
        tagged::<FileChange>(),
        tagged::<TagAnswer>(),
        tagged::<PinAnswer>(),
        tagged::<RevertAnswer>(),
        tagged::<PurgeAnswer>(),
        tagged::<Prepared>(),
        tagged::<DreamEnd>(),
        tagged::<FilesAnswer>(),
        tagged::<SaveAnswer>(),
        tagged::<ReadAnswer>(),
        tagged::<TagRefusal>(),
        tagged::<BadPath>(),
        tagged::<Protected>(),
        tagged::<TooLong>(),
        tagged::<TooLarge>(),
        tagged::<Missing>(),
        tagged::<NotAHire>(),
        tagged::<BadCommit>(),
        tagged::<Gone>(),
        tagged::<DreamOpen>(),
        named::<Removal>(),
    ]
}

pub fn unions() -> Vec<Union> {
    vec![
        union!(ReadFileRefusal {
            Missing: Missing,
            Commit: BadCommit,
            Path: BadPath,
        }),
        union!(PinRefusal {
            Missing: Missing,
            Hire: NotAHire,
        }),
        union!(RevertRefusal {
            Missing: Missing,
            Commit: BadCommit,
        }),
        union!(EditRefusal {
            Path: BadPath,
            Protected: Protected,
            Size: TooLong,
        }),
        union!(PurgeRefusal {
            Path: BadPath,
            Dream: DreamOpen,
        }),
        union!(SaveRefusal {
            Path: BadPath,
            Protected: Protected,
            Size: TooLarge,
            Gone: Gone,
        }),
    ]
}

pub fn the_profiles_messages(fixtures: &mut Fixtures) {
    the_input(fixtures);
    the_versions(fixtures);
    the_reads(fixtures);
    the_changes_of_main(fixtures);
    the_dreams(fixtures);
    the_files_of_a_hire(fixtures);
    the_calls_on_an_agent(fixtures);
}

const HIRE: AgentNumber = AgentNumber(12);

fn commit(byte: u8) -> CommitId {
    CommitId(Sha1([byte; 20]))
}

fn version() -> VersionName {
    VersionName("v1.2".into())
}

pub fn request() -> RequestId {
    "6f1c2a3e-8b4d-4c5e-9f60-718293a4b5c6".parse().unwrap()
}

fn chief() -> By {
    By::Chief { chief: 1 }
}

fn dream() -> DreamId {
    DreamId(3)
}

fn character() -> Character {
    Character {
        version: 2,
        text: "You write careful tests.".into(),
    }
}

fn piece(path: &str) -> Piece {
    Piece {
        path: path.into(),
        offset: 0,
        bytes: Base64(b"# Notes\n".to_vec()),
        last: true,
        executable: false,
    }
}

fn no_version() -> Missing {
    Missing::NoVersion {
        profile: ProfileId(4),
        version: version(),
    }
}

fn not_a_path() -> BadPath {
    BadPath::NotAPath {
        path: "../secrets".into(),
    }
}

fn withheld() -> Protected {
    Protected::Withheld {
        path: "inbox/letter.md".into(),
    }
}

fn the_input(fixtures: &mut Fixtures) {
    for (name, input) in [
        (
            "character",
            ProfileInput::Character {
                text: "You write careful tests.".into(),
            },
        ),
        (
            "fork",
            ProfileInput::Fork {
                profile: ProfileId(2),
                at: commit(0xab),
            },
        ),
    ] {
        fixtures.input(&format!("profile/{name}"), &input);
        fixtures.stored(&format!("profile.input.{name}"), &input);
    }
}

fn the_versions(fixtures: &mut Fixtures) {
    let tag = |at| Tag {
        id: request(),
        version: version(),
        at,
        note: "Before the refactor".into(),
        by: chief(),
    };
    fixtures.args("profile/tag", &tag(Some(commit(0xab))));
    fixtures.args("profile/tag.main", &tag(None));
    for (arm, answer) in [
        (
            "tagged",
            TagAnswer::Tagged {
                version: version(),
                commit: commit(0xab),
            },
        ),
        (
            "refused.bad_name",
            TagAnswer::refused(TagRefusal::BadName {
                version: VersionName("v 1".into()),
            }),
        ),
        (
            "refused.taken",
            TagAnswer::refused(TagRefusal::Taken {
                version: version(),
            }),
        ),
        (
            "refused.no_such_commit",
            TagAnswer::refused(TagRefusal::NoSuchCommit {
                profile: ProfileId(4),
                commit: commit(0xab),
            }),
        ),
    ] {
        fixtures.kept(&format!("profile/tag.{arm}"), &answer);
    }
    fixtures.args("profile/versions", &Versions);
    fixtures.answer(
        "profile/versions",
        &VersionList {
            versions: vec![VersionSeen {
                version: version(),
                commit: commit(0xab),
                note: "Before the refactor".into(),
                by: "chief:1".into(),
            }],
            commits: vec![CommitSeen {
                commit: commit(0xab),
                message: "Edited the memory".into(),
                by: "chief:1".into(),
            }],
        },
    );
    let pin = |to| Pin {
        id: request(),
        hire: HIRE,
        to,
        by: By::Agent { agent: AGENT },
    };
    fixtures.args("profile/pin", &pin(Some(version())));
    fixtures.args("profile/pin.main", &pin(None));
    for (arm, answer) in [
        (
            "pinned",
            PinAnswer::Pinned {
                version: version(),
                commit: commit(0xab),
            },
        ),
        ("follows_main", PinAnswer::FollowsMain),
        (
            "refused.no_version",
            PinAnswer::refused(no_version().into()),
        ),
        (
            "refused.no_hire",
            PinAnswer::refused(
                NotAHire::NoHire { hire: HIRE }.into(),
            ),
        ),
    ] {
        fixtures.kept(&format!("profile/pin.{arm}"), &answer);
    }
}

fn the_reads(fixtures: &mut Fixtures) {
    fixtures.args(
        "profile/read",
        &Read {
            at: None,
            path: None,
        },
    );
    fixtures.args(
        "profile/read.file",
        &Read {
            at: Some(Revision::Commit {
                commit: commit(0xab),
            }),
            path: Some("memory/tests.md".into()),
        },
    );
    for (arm, answer) in [
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
                cut: true,
            },
        ),
        (
            "refused.no_version",
            ReadAnswer::refused(no_version().into()),
        ),
        (
            "refused.no_commit",
            ReadAnswer::refused(
                BadCommit::NoCommit {
                    profile: ProfileId(4),
                    commit: commit(0xab),
                }
                .into(),
            ),
        ),
        (
            "refused.not_on_main",
            ReadAnswer::refused(
                BadCommit::NotOnMain {
                    commit: commit(0xab),
                }
                .into(),
            ),
        ),
        (
            "refused.not_a_path",
            ReadAnswer::refused(not_a_path().into()),
        ),
    ] {
        let answer: ReadAnswer = answer;
        fixtures.answer(&format!("profile/read.{arm}"), &answer);
    }
}

fn the_changes_of_main(fixtures: &mut Fixtures) {
    fixtures.args(
        "profile/revert",
        &Revert {
            id: request(),
            what: Revision::Version { version: version() },
            reason: None,
            by: chief(),
        },
    );
    fixtures.args(
        "profile/revert.commit",
        &Revert {
            id: request(),
            what: Revision::Commit {
                commit: commit(0xab),
            },
            reason: Some("It forgot the tests".into()),
            by: chief(),
        },
    );
    fixtures.args(
        "profile/revert.main",
        &Revert {
            id: request(),
            what: Revision::Main,
            reason: None,
            by: chief(),
        },
    );
    for (arm, answer) in [
        (
            "reverted",
            RevertAnswer::Reverted {
                commit: commit(0xcd),
                clashes: vec![Path {
                    path: "memory/tests.md".into(),
                }],
            },
        ),
        ("unchanged", RevertAnswer::<RevertRefusal>::Unchanged),
        (
            "refused.no_version",
            RevertAnswer::refused(no_version().into()),
        ),
        (
            "refused.no_commit",
            RevertAnswer::refused(
                BadCommit::NoCommit {
                    profile: ProfileId(4),
                    commit: commit(0xab),
                }
                .into(),
            ),
        ),
        (
            "refused.not_on_main",
            RevertAnswer::refused(
                BadCommit::NotOnMain {
                    commit: commit(0xab),
                }
                .into(),
            ),
        ),
    ] {
        fixtures.kept(&format!("profile/revert.{arm}"), &answer);
    }
    fixtures.args(
        "profile/edit",
        &Edit {
            id: request(),
            path: "CHARACTER.md".into(),
            text: "You write careful tests.".into(),
            message: None,
            by: chief(),
        },
    );
    fixtures.args(
        "profile/edit.message",
        &Edit {
            id: request(),
            path: "skills/tidy/SKILL.md".into(),
            text: String::new(),
            message: Some("Took out the tidy skill".into()),
            by: By::Agent { agent: AGENT },
        },
    );
    for (arm, answer) in [
        ("done", Done::Done),
        ("refused.not_a_path", Done::refused(not_a_path().into())),
        ("refused.withheld", Done::refused(withheld().into())),
        (
            "refused.character_too_long",
            Done::refused(EditRefusal::from(
                TooLong::CharacterTooLong { most: 4000 },
            )),
        ),
    ] {
        fixtures.kept(&format!("profile/edit.{arm}"), &answer);
    }
    fixtures.args(
        "profile/purge",
        &Purge {
            id: request(),
            path: "memory/password.md".into(),
            by: chief(),
        },
    );
    for (arm, answer) in [
        (
            "purged",
            PurgeAnswer::Purged {
                branches: 3,
                still_hold: vec![HIRE],
            },
        ),
        (
            "refused.not_a_path",
            PurgeAnswer::refused(not_a_path().into()),
        ),
        (
            "refused.dreaming",
            PurgeAnswer::refused(
                DreamOpen::Dreaming { dream: dream() }.into(),
            ),
        ),
    ] {
        let answer: PurgeAnswer = answer;
        fixtures.kept(&format!("profile/purge.{arm}"), &answer);
    }
    fixtures.args("profile/erase", &Erase { id: request() });
    for (arm, answer) in [
        ("done", Done::Done),
        (
            "refused.dreaming",
            Done::refused(DreamOpen::Dreaming { dream: dream() }),
        ),
    ] {
        fixtures.kept(&format!("profile/erase.{arm}"), &answer);
    }
    fixtures.args(
        "profile/drop_save_parts",
        &DropSaveParts { agent: AGENT },
    );
    fixtures.answer("profile/drop_save_parts", &());
    fixtures.args("profile/papers", &Papers);
    fixtures.answer(
        "profile/papers",
        &PaperList {
            character: "You write careful tests.".into(),
            memory: vec![Paper {
                path: "memory/tests.md".into(),
                description: "How the tests run".into(),
            }],
            skills: vec![Paper {
                path: "skills/fixtures/SKILL.md".into(),
                description: "Writes fixtures".into(),
            }],
        },
    );
}

fn the_dreams(fixtures: &mut Fixtures) {
    for (name, of) in [
        ("hire", DreamOf::Hire { agent: HIRE }),
        ("profile", DreamOf::Profile),
    ] {
        fixtures.args(
            &format!("profile/prepare_dream.{name}"),
            &PrepareDream { dream: dream(), of },
        );
    }
    for (arm, answer) in [
        (
            "ready.hire",
            Prepared::Ready {
                main: "dreams/3/main".into(),
                hire: Some("dreams/3/hire".into()),
            },
        ),
        (
            "ready.profile",
            Prepared::Ready {
                main: "dreams/3/main".into(),
                hire: None,
            },
        ),
        ("nothing", Prepared::Nothing),
    ] {
        fixtures
            .answer(&format!("profile/prepare_dream.{arm}"), &answer);
    }
    fixtures.args(
        "profile/end_dream",
        &EndDream {
            dream: dream(),
            done: true,
            summary: "Moved the notes on tests to memory".into(),
            hire_name: Some("hire#3 (ada)".into()),
        },
    );
    for (arm, answer) in [
        (
            "dreamt",
            DreamEnd::Dreamt {
                main: Some(commit(0xcd)),
            },
        ),
        ("dreamt.main_unchanged", DreamEnd::Dreamt { main: None }),
        ("nothing", DreamEnd::Nothing),
    ] {
        fixtures.kept(&format!("profile/end_dream.{arm}"), &answer);
    }
}

fn the_files_of_a_hire(fixtures: &mut Fixtures) {
    let cursor = || Cursor {
        commit: commit(0xab),
        file: 4,
        offset: 65536,
    };
    for (name, since, after) in [
        ("tip", None, None),
        ("since", Some(commit(0x12)), Some(cursor())),
    ] {
        fixtures.args(
            &format!("profile/files.{name}"),
            &Files { since, after },
        );
    }
    for (arm, answer) in [
        (
            "part",
            FilesAnswer::Part {
                tip: commit(0xab),
                files: vec![
                    piece("memory/tests.md"),
                    Piece {
                        path: "skills/run/run.sh".into(),
                        offset: 0,
                        bytes: Base64(b"#!/bin/sh\n".to_vec()),
                        last: false,
                        executable: true,
                    },
                ],
                removed: Vec::new(),
                next: Some(cursor()),
                character: Some(character()),
            },
        ),
        (
            "part.last",
            FilesAnswer::Part {
                tip: commit(0xab),
                files: Vec::new(),
                removed: vec![Path {
                    path: "memory/old.md".into(),
                }],
                next: None,
                character: None,
            },
        ),
        (
            "refused.commit_gone",
            FilesAnswer::refused(Gone::CommitGone {
                commit: commit(0x12),
            }),
        ),
    ] {
        fixtures.answer(&format!("profile/files.{arm}"), &answer);
    }
    let save = |part, last| Save {
        call: call("call-6"),
        base: commit(0xab),
        part,
        last,
        changes: vec![
            FileChange::Write(piece("memory/tests.md")),
            FileChange::Remove {
                path: "memory/old.md".into(),
            },
        ],
    };
    fixtures.args("profile/save", &save(0, false));
    fixtures.args("profile/save.last", &save(1, true));
    for (arm, answer) in [
        ("saved", SaveAnswer::Saved { tip: commit(0xcd) }),
        ("kept", SaveAnswer::Kept),
        (
            "refused.not_a_path",
            SaveAnswer::refused(not_a_path().into()),
        ),
        ("refused.withheld", SaveAnswer::refused(withheld().into())),
        (
            "refused.part_too_large",
            SaveAnswer::refused(SaveRefusal::from(
                TooLarge::PartTooLarge {
                    most: MOST_PART as u32,
                },
            )),
        ),
        (
            "refused.commit_gone",
            SaveAnswer::refused(
                Gone::CommitGone {
                    commit: commit(0xab),
                }
                .into(),
            ),
        ),
    ] {
        fixtures.kept(&format!("profile/save.{arm}"), &answer);
    }
}

fn the_calls_on_an_agent(fixtures: &mut Fixtures) {
    fixtures.args(
        "profile/moved",
        &Moved {
            tip: commit(0xcd),
            character: Some(character()),
        },
    );
    fixtures.args(
        "profile/moved.tip",
        &Moved {
            tip: commit(0xcd),
            character: None,
        },
    );
    fixtures.answer("profile/moved", &());
    fixtures.args(
        "profile/remove_path",
        &RemovePath {
            path: "memory/password.md".into(),
        },
    );
    for (arm, answer) in
        [("removed", Removal::Removed), ("later", Removal::Later)]
    {
        fixtures
            .answer(&format!("profile/remove_path.{arm}"), &answer);
    }
}

#[test]
fn a_piece_reads_a_null_executable_as_not_running() {
    let piece = serde_json::json!({
        "path": "run.sh",
        "offset": 0,
        "bytes": "",
        "last": true,
        "executable": null,
    });
    let piece: Piece = serde_json::from_value(piece).unwrap();
    assert!(!piece.executable);
}
