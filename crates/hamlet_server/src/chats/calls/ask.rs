//! An agent's question to the chief with `ask_chief`, which waits for
//! the answer.

use anyhow::Result;
use hamlet_protocol::{Choice, Questions};

use super::super::{asking, records};
use super::WAITING;
use super::decide::{Admitted, chat_of, wait_by};
use crate::harness::Answer;
use crate::oversight::held::{self, CallState};
use crate::oversight::{Holding, Raising, Ruling, WaitKind};
use crate::service::Service;

/// The words of a question no chief can answer.
const NO_CHIEF: &str =
    "No chief oversees this turn; go on with what you know.";

/// Asks the chief the questions of `call`, or gives the chief's
/// answer once there is one.
pub(super) fn ask(
    service: &mut Service,
    call: &Admitted<'_>,
) -> Result<Answer> {
    let binding = call.bound.id;
    if service.home.store().cancelling(binding)? {
        return Ok(Answer::Refusal(
            crate::harness::answers::STOPPED_WORDS.into(),
        ));
    }
    let answered = |text: &str| Ok(Answer::result(text, false));
    let db = service.home.store().connection();
    match held::decision(db, &call.id)?.map(|kept| kept.state) {
        Some(CallState::Blocked(words)) => {
            return Ok(Answer::Refusal(words));
        }
        Some(CallState::Ruled {
            ruling: Ruling::Reply,
            text,
            structured,
            ..
        }) => {
            return if structured {
                answered(&text)
            } else {
                answered(&format!("{} {text}", asking::ANSWERS))
            };
        }
        Some(CallState::Ruled { .. }) => {
            return answered(
                "The chief did not answer; go on with what you know.",
            );
        }
        Some(CallState::Held { .. }) => {
            return Ok(Answer::Waiting(WAITING.into()));
        }
        // An allowance is the gate's, made before the question.
        Some(CallState::Open | CallState::Allowed { .. }) | None => {}
    }
    let Some(conversation) = records::kept(db, binding)? else {
        return answered(NO_CHIEF);
    };
    let place = conversation.place;
    if !super::chief_oversees(service, binding, call.input, &place)? {
        return answered(NO_CHIEF);
    }
    let questions =
        match asking::read(&serde_json::to_string(call.arguments)?) {
            Ok(questions) => questions,
            Err(why) => return answered(&why),
        };
    let mut questions = serde_json::to_value(questions)?;
    service.credentials.scrub(&mut questions);
    let questions = serde_json::from_value::<Questions>(questions)?;
    let most =
        service.limits_of(&place.agent.hamlet)?.asking.questions;
    let db = service.home.store().connection();
    let mut asked = crate::gate::Asked::of(db, binding, call.input)?;
    if asked.questions >= most {
        let times = if most == 1 {
            "once".into()
        } else {
            format!("{most} times")
        };
        return answered(&format!(
            "You have asked the chief {times} this turn; go on with \
             what you know."
        ));
    }
    asked.questions += 1;
    asked.keep(db, binding)?;
    let Some(record) =
        service.home.store().record(&place.agent.hamlet)?
    else {
        return answered(NO_CHIEF);
    };
    held::raise(
        service.home.store().connection(),
        Raising {
            about: asking::about(&questions),
            kind: WaitKind::Question(questions),
            by: wait_by(&record, &place.agent),
            agent: Some(place.agent.clone()),
            change: None,
            answers: vec![Choice::Reply, Choice::Dismiss],
            chat: chat_of(&place),
            call: Some(Holding {
                id: call.id.clone(),
                binding,
                input: call.input.map(String::from),
                change: None,
            }),
        },
        crate::clock::now(),
    )?;
    Ok(Answer::Waiting(WAITING.into()))
}
