//! The fixtures of the prompt's sections and of the calls the realm
//! makes to agents and profiles.

use hamlet_messages::address::{Party, Profile};
use hamlet_messages::agent::{
    AgentInput, AgentKind, Author, Brief, BuiltIn, DropInbox, Erase,
    Fill, HeldCall, HeldEnd, InboxCursor, InboxPart, Input, InputId,
    OnTask, Place, PlaceVersion, ReadInbox, Retire, Retired, Submit,
    TakeInbox,
};
use hamlet_messages::bytes::Base64;
use hamlet_messages::profile::{
    Branch, CommitId, Missing, Piece, Sha1, VersionName,
};
use hamlet_messages::realm::prompt::{
    ConversationKind, PlaceText, ReadPlace,
};
use hamlet_messages::realm::{LetterId, ProfileId, ReadOwnPlace};
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, RealmTask,
};

use super::super::{AGENT, Fixtures, task};
use super::{
    HIRE, MAYOR, done, named_tool, parcel_file, standing_task,
};

pub(super) fn the_prompt_sections(fixtures: &mut Fixtures) {
    for (name, conversation, peer) in [
        ("realm/read_place", ConversationKind::Chat, None),
        (
            "realm/read_place.letters",
            ConversationKind::Letters,
            Some(HIRE),
        ),
        (
            "realm/read_place.questions",
            ConversationKind::Questions,
            Some(HIRE),
        ),
        ("realm/read_place.other", ConversationKind::Other, None),
    ] {
        fixtures.args(name, &ReadPlace { conversation, peer });
    }
    fixtures.answer(
        "realm/read_place",
        &PlaceText {
            place: "# Your place\nYou are `hire#12 (Ada)`.".into(),
            realm: Some(
                "# The realm\nNo hamlet is founded yet.".into(),
            ),
            profiles: Some(
                "# The city's profiles\n- `profile#2`".into(),
            ),
        },
    );
    fixtures.answer(
        "realm/read_place.place_only",
        &PlaceText {
            place: "# Your place\nYou are `hand`.".into(),
            realm: None,
            profiles: None,
        },
    );
}

pub(super) fn place() -> Place {
    Place {
        version: PlaceVersion(4),
        lead: Some(MAYOR),
        hires: vec![AgentNumber(13)],
        roads: vec![AgentNumber(14)],
        tools_off: vec![named_tool("found"), named_tool("bash")],
        folder: Some("/work/orchard".into()),
        manual: true,
        agrees_roads: false,
    }
}

pub(super) fn the_calls_to_agents_and_profiles(
    fixtures: &mut Fixtures,
) {
    let hands_place = || Place {
        version: PlaceVersion(1),
        lead: None,
        hires: Vec::new(),
        roads: Vec::new(),
        tools_off: Vec::new(),
        folder: None,
        manual: false,
        agrees_roads: false,
    };
    for (name, kind, place, copies) in [
        ("hand", AgentKind::Hand, hands_place(), Default::default()),
        (
            "hire",
            AgentKind::Hire,
            place(),
            super::super::chat::copies(),
        ),
    ] {
        fixtures.input(
            &format!("agent/{name}"),
            &AgentInput {
                kind,
                task: standing_task(),
                place,
                brief: None,
                copies,
            },
        );
    }
    for (name, kind, brief) in [
        (
            "worker",
            AgentKind::Worker,
            Brief {
                instructions: None,
                text: "Check the links in the docs".into(),
                tools: vec![BuiltIn {
                    name: "read".into(),
                }],
                model: None,
                dream: false,
            },
        ),
        (
            "errand",
            AgentKind::Errand,
            Brief {
                instructions: Some("Review the chief's line.".into()),
                text: "Write the tests".into(),
                tools: Vec::new(),
                model: Some("faux::faux-1".into()),
                dream: false,
            },
        ),
        (
            "dream",
            AgentKind::Errand,
            Brief {
                instructions: Some("You are dreaming.".into()),
                text: "Dream now.".into(),
                tools: Vec::new(),
                model: Some("faux::faux-1".into()),
                dream: true,
            },
        ),
    ] {
        fixtures.input(
            &format!("agent/{name}"),
            &AgentInput {
                kind,
                task: standing_task(),
                place: hands_place(),
                brief: Some(brief),
                copies: Default::default(),
            },
        );
    }
    let author = || Author {
        agent: HIRE,
        name: Some("Ada".into()),
    };
    for (name, task, input) in [
        (
            "line",
            task(),
            Input::Line {
                text: "Write the tests".into(),
                images: Vec::new(),
                files: Vec::new(),
                note: None,
                chief: Some("ada".into()),
            },
        ),
        (
            "line.reviewed",
            task(),
            Input::Line {
                text: "Write the tests".into(),
                images: Vec::new(),
                files: Vec::new(),
                note: Some("hire#2 reviewed this line: fine".into()),
                chief: None,
            },
        ),
        (
            "notice",
            task(),
            Input::Notice {
                text: "The chief answers: use the staging server"
                    .into(),
                conversation: Some(ConversationId("c1".into())),
                held: Some(HeldCall {
                    call: CallId("t7".into()),
                    end: HeldEnd::Ran,
                }),
            },
        ),
        (
            "notice.blocked",
            task(),
            Input::Notice {
                text: "the chief refused it: keep it".into(),
                conversation: Some(ConversationId("c1".into())),
                held: Some(HeldCall {
                    call: CallId("t8".into()),
                    end: HeldEnd::Blocked,
                }),
            },
        ),
        (
            "notice.aborted",
            task(),
            Input::Notice {
                text: "the call's wait for a chief lapsed: the turn \
                       was stopped"
                    .into(),
                conversation: Some(ConversationId("c1".into())),
                held: Some(HeldCall {
                    call: CallId("t9".into()),
                    end: HeldEnd::Aborted,
                }),
            },
        ),
        (
            "notice.withdrawn",
            task(),
            Input::Notice {
                text: "The chief withdrew the instruction: Make the \
                       file."
                    .into(),
                conversation: Some(ConversationId("c1".into())),
                held: Some(HeldCall {
                    call: CallId("t10".into()),
                    end: HeldEnd::Withdrawn,
                }),
            },
        ),
        (
            "worker_answer",
            task(),
            Input::WorkerAnswer {
                from: Author {
                    agent: AgentNumber(15),
                    name: Some("worker#15".into()),
                },
                text: "Three links are broken".into(),
                conversation: Some(ConversationId("c1".into())),
            },
        ),
        (
            "line.attached",
            task(),
            Input::Line {
                text: "Look at this plan".into(),
                images: vec![super::super::parity::line_image()],
                files: vec![super::super::parity::line_file()],
                note: None,
                chief: None,
            },
        ),
        (
            "letter",
            None,
            Input::Letter {
                from: author(),
                text: "The tests pass".into(),
                files: Vec::new(),
            },
        ),
        (
            "letter.files",
            None,
            Input::Letter {
                from: author(),
                text: "The report".into(),
                files: vec![
                    parcel_file(0, "from-hire-12/report.md", 2_048),
                    parcel_file(
                        1,
                        "from-hire-12/figures/plot.png",
                        6 << 30,
                    ),
                ],
            },
        ),
        (
            "question",
            task(),
            Input::Question {
                letter: LetterId(5),
                from: Author {
                    agent: AGENT,
                    name: None,
                },
                text: "Which tests fail?".into(),
                files: Vec::new(),
            },
        ),
        (
            "question.files",
            task(),
            Input::Question {
                letter: LetterId(6),
                from: author(),
                text: "Is this plan right?".into(),
                files: vec![parcel_file(
                    0,
                    "from-hire-12/plan.md",
                    512,
                )],
            },
        ),
        (
            "answer",
            task(),
            Input::Answer {
                question: LetterId(5),
                from: author(),
                text: "Two of them".into(),
                conversation: ConversationId("c1".into()),
            },
        ),
    ] {
        fixtures.args(
            &format!("agent/submit.{name}"),
            &Submit {
                id: InputId("input-1".into()),
                task,
                input,
            },
        );
    }
    fixtures.answer("agent/submit", &());
    fixtures.args("agent/place", &place());
    fixtures.args("agent/place.hand", &hands_place());
    fixtures.answer("agent/place", &());
    fixtures.args("realm/read_own_place", &ReadOwnPlace);
    fixtures.answer("realm/read_own_place", &place());
    fixtures.args(
        "agent/fill",
        &Fill {
            profile: ProfileId(2),
        },
    );
    fixtures.answer("agent/fill", &());
    fixtures.args("agent/retire", &Retire);
    fixtures.answer(
        "agent/retire",
        &Retired {
            tip: CommitId(Sha1([0xcd; 20])),
        },
    );
    fixtures.args("agent/erase", &Erase);
    fixtures.answer("agent/erase", &());
    fixtures.args("agent/read_inbox", &ReadInbox { after: None });
    let cursor = || InboxCursor {
        path: "scan.pdf".into(),
        offset: 737_216,
    };
    fixtures.args(
        "agent/read_inbox.after",
        &ReadInbox {
            after: Some(cursor()),
        },
    );
    let letter = || Piece {
        path: "from-chief/letter.md".into(),
        offset: 0,
        bytes: Base64(b"a letter\n".to_vec()),
        last: true,
        executable: false,
    };
    fixtures.answer(
        "agent/read_inbox",
        &InboxPart {
            files: vec![letter()],
            next: Some(cursor()),
        },
    );
    fixtures.answer(
        "agent/read_inbox.last",
        &InboxPart {
            files: vec![letter()],
            next: None,
        },
    );
    fixtures.args(
        "agent/take_inbox",
        &TakeInbox {
            from: AgentNumber(4),
            files: vec![letter()],
        },
    );
    fixtures.answer("agent/take_inbox", &());
    fixtures.args(
        "agent/drop_inbox",
        &DropInbox {
            paths: vec!["from-chief/letter.md".into()],
        },
    );
    fixtures.answer("agent/drop_inbox", &());
    fixtures.args(
        "agent/on_task",
        &OnTask {
            tasks: vec![RealmTask("7".into())],
            within_ms: 300_000,
        },
    );
    fixtures.answer("agent/on_task", &true);
    fixtures.args(
        "profile/branch",
        &Branch {
            name: "hires/12".into(),
            from: None,
        },
    );
    fixtures.args(
        "profile/branch.from",
        &Branch {
            name: "hires/12".into(),
            from: Some(VersionName("v1".into())),
        },
    );
    done(
        fixtures,
        "profile/branch",
        vec![(
            "no_version",
            Missing::NoVersion {
                profile: ProfileId(2),
                version: VersionName("v1".into()),
            },
        )],
    );
    fixtures.answer(
        "address/profile",
        &Profile(ProfileId(2)).address().unwrap(),
    );
}
