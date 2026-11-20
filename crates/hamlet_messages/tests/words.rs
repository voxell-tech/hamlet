//! The refusals a model or a chief reads, each in the words today's
//! server gives for the same case, with the line they are taken from.

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
fn each_refusal_reads_as_todays_server_words_it() {
    let ended = |how| Left {
        how,
        why: Some("not needed".to_owned()),
    };
    let mill = Payer::Hamlet {
        hamlet: HamletId("mill".to_owned()),
    };
    let cases = [
        // hamlet_server/src/society/refusal.rs:141
        (
            words(ResumeRefusal::NoSuchSession { number: 9 }),
            "there is no past session 9 here",
        ),
        // hamlet_server/src/society/refusal.rs:143,
        // talk/chats.rs:387
        (
            words(RetryRefusal::Running),
            "a turn is running here; stop it, then retry",
        ),
        // hamlet_server/src/society/refusal.rs:145
        (
            words(RetryRefusal::Answered),
            "the last message was answered, so there is nothing to \
             carry on",
        ),
        // hamlet_server/src/talk/chats.rs:317
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
        // hamlet_server/src/clients/requests.rs:408
        (
            words(RewindRefusal::NotCurrent),
            "Only the current session of a chief's chat goes back.",
        ),
        // hamlet_server/src/society/refusal.rs:143,
        // talk/chats.rs:323
        (
            words(RewindRefusal::Working),
            "a turn is running here; stop it, then rewind",
        ),
        // hamlet_server/src/calls.rs:43
        (
            words(Gate::Reserved { wait: WaitId(4) }),
            "This call waits for a chief's answer.",
        ),
        // hamlet_server/src/society/refusal.rs:88,
        // society/agents.rs:114
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
        // hamlet_server/src/society/agents.rs:119
        (words(ended(HowEnded::Failed)), "its task failed"),
        // hamlet_server/src/calls/society/letters.rs:128
        (words(LetterRefusal::Yourself), "That is you."),
        // hamlet_server/src/calls/society/letters.rs:133
        (words(LetterRefusal::Empty), "Say what to send."),
        // hamlet_server/src/calls/society/letters.rs:141
        (
            words(LetterRefusal::NoRoad { to: mira() }),
            "You have no road to hire#12 (Mira). Ask your lead for \
             one with request_road.",
        ),
        // hamlet_server/src/commands.rs:1592, society/refusal.rs:107
        (words(EndRefusal::TheHand), "`hand` is not a hire"),
        // hamlet_server/src/society/roads.rs:201,
        // society/refusal.rs:111
        (
            words(LayRefusal::SameEnds),
            "a road needs two different hires",
        ),
        // hamlet_server/src/society/roads.rs:206,
        // society/refusal.rs:139
        (words(LayRefusal::NotHires), "a road joins hires only"),
        // hamlet_server/src/society/roads.rs:212,
        // society/refusal.rs:103
        (
            words(LayRefusal::OtherHamlet { hire: mira() }),
            "`hire#12 (Mira)` is in another hamlet",
        ),
        // hamlet_server/src/calls/society/letters.rs:70
        (
            words(RequestRoadRefusal::NoLead),
            "a road joins hires only",
        ),
        // hamlet_server/src/spend.rs:224-235
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
        // hamlet_server/src/spend.rs:236-242, spend.rs:57-66
        (
            words(Budget::OverTime {
                payer: mill,
                spent_ms: 4_000_000,
                budget_ms: 3_900_000,
            }),
            "The hamlet `mill`'s budget of 1h 05m of model time is \
             spent.",
        ),
        // hamlet_server/src/calls/objections.rs:19
        (
            words(Hold::Objection {
                task: RealmTask("t7".to_owned()),
            }),
            "The chief is considering an objection.",
        ),
        // hamlet_server/src/calls.rs:55
        (
            words(AskChiefRefusal::NoChief),
            "No chief oversees this turn; go on with what you know.",
        ),
        // hamlet_server/src/calls.rs:1135-1143
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
        // hamlet_server/src/calls/objections.rs:123
        (
            words(ObjectRefusal::NotAChiefsLine),
            "Only a chief's instruction can be objected to, in the \
             turn that works on it.",
        ),
        // hamlet_server/src/calls/objections.rs:172
        (words(ObjectRefusal::NoReason), "Say why you object."),
    ];

    for (given, today) in cases {
        assert_eq!(given, today);
    }
}
