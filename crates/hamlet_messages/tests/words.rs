//! The refusals a model or a chief reads, each in its words.

use std::fmt::Display;

use hamlet_messages::agent::chat::{
    ResumeRefusal, RetryRefusal, RewindRefusal,
};
use hamlet_messages::realm::budget::{Budget, Payer};
use hamlet_messages::realm::oversight::{
    AskChiefRefusal, Hold, ObjectRefusal,
};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{
    AgentName, EndRefusal, Gate, HamletId, HowEnded, LayRefusal,
    Left, LetterRefusal, RequestRoadRefusal,
};
use hamlet_messages::workspace::{AgentNumber, RealmTask};

fn mira() -> AgentName {
    AgentName {
        agent: AgentNumber(12),
        name: "hire#12 (Mira)".to_owned(),
    }
}

fn words(refusal: impl Display) -> String {
    refusal.to_string()
}

#[test]
fn each_refusal_reads_in_its_words() {
    let ended = |how| Left {
        how,
        why: Some("not needed".to_owned()),
    };
    let mill = Payer::Hamlet {
        hamlet: HamletId("mill".to_owned()),
    };
    let cases = [
        (
            words(ResumeRefusal::NoSuchSession { number: 9 }),
            "there is no past session 9 here",
        ),
        (
            words(RetryRefusal::Running),
            "a turn is running here; stop it, then retry",
        ),
        (
            words(RetryRefusal::Answered),
            "the last message was answered, so there is nothing to \
             carry on",
        ),
        (
            words(RewindRefusal::NotAChiefLine),
            "That is not one of your lines in this conversation.",
        ),
        (
            words(RewindRefusal::NoSuchItem {
                item: "i9".to_owned(),
            }),
            "That is not one of your lines in this conversation.",
        ),
        (
            words(RewindRefusal::NotCurrent),
            "Only the current session of a chief's chat goes back.",
        ),
        (
            words(RewindRefusal::Working),
            "a turn is running here; stop it, then rewind",
        ),
        (
            words(Gate::Reserved { wait: WaitId(4) }),
            "This call waits for a chief's answer.",
        ),
        (
            words(EndRefusal::Left {
                hire: mira(),
                left: ended(HowEnded::ByTheChief),
            }),
            "`hire#12 (Mira)` has left: it was ended",
        ),
        (
            words(ended(HowEnded::ByALead { lead: mira() })),
            "it was ended",
        ),
        (words(ended(HowEnded::ByTheHand)), "it was ended"),
        (words(ended(HowEnded::Failed)), "its task failed"),
        (
            words(ended(HowEnded::Merged {
                into: mira(),
                by: None,
            })),
            "it was merged into another hire",
        ),
        (
            words(ended(HowEnded::ProfileArchived)),
            "its scholar left the city",
        ),
        (
            words(ended(HowEnded::HamletRemoved { by: None })),
            "its hamlet was removed",
        ),
        (words(LetterRefusal::Yourself), "That is you."),
        (words(LetterRefusal::Empty), "Say what to send."),
        (
            words(LetterRefusal::NoRoad { to: mira() }),
            "You have no road to hire#12 (Mira). Ask your lead for \
             one with request_road.",
        ),
        (words(EndRefusal::TheHand), "`hand` is not a hire"),
        (
            words(LayRefusal::SameEnds),
            "a road needs two different hires",
        ),
        (words(LayRefusal::NotHires), "a road joins hires only"),
        (
            words(LayRefusal::OtherHamlet { hire: mira() }),
            "`hire#12 (Mira)` is in another hamlet",
        ),
        (
            words(RequestRoadRefusal::NoLead),
            "a road joins hires only",
        ),
        (
            words(Budget::OverCost {
                payer: mill.clone(),
                spent_micros: 2_500_000,
                budget_micros: 2_000_000,
            }),
            "The hamlet `mill`'s budget of $2.00 is spent. A chief \
             can raise it, or start its period again with \
             /reset-budget.",
        ),
        (
            words(Budget::OverCost {
                payer: Payer::Realm,
                spent_micros: 1,
                budget_micros: 0,
            }),
            "The realm's budget of $0.00 is spent. A chief can \
             raise it, or start its period again with /reset-budget.",
        ),
        (
            words(Budget::OverTime {
                payer: mill,
                spent_ms: 4_000_000,
                budget_ms: 3_900_000,
            }),
            "The hamlet `mill`'s budget of 1h 05m of model time is \
             spent.",
        ),
        (
            words(Hold::Objection {
                task: RealmTask("t7".to_owned()),
            }),
            "The chief is considering an objection.",
        ),
        (
            words(AskChiefRefusal::NoChief),
            "No chief oversees this turn; go on with what you know.",
        ),
        (
            words(AskChiefRefusal::TooMany { most: 1 }),
            "You have asked the chief once this turn; go on with \
             what you know.",
        ),
        (
            words(AskChiefRefusal::TooMany { most: 3 }),
            "You have asked the chief 3 times this turn; go on with \
             what you know.",
        ),
        (
            words(ObjectRefusal::NotAChiefsLine),
            "Only a chief's instruction can be objected to, in the \
             turn that works on it.",
        ),
        (words(ObjectRefusal::NoReason), "Say why you object."),
    ];

    for (given, expected) in cases {
        assert_eq!(given, expected);
    }
}
