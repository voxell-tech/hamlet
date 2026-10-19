use bevy::prelude::*;
use hamlet_core::tools::AgentToolCalled;
use hamlet_core::tools::plugged::Waiting;
use hamlet_sdk::capability::Refusal;

use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::limits::Limits;
use crate::manifest::Manifest;

fn probe() -> Plugin {
    Plugin::new("probe", fixture::component(&fixture::echo()))
}

fn call(app: &mut App) -> Waiting {
    let (mut called, waiting) = AgentToolCalled::new(
        "Juniper",
        "Home",
        "probe",
        "look",
        vec![],
    );
    app.world_mut().trigger_ref(&mut called);
    app.world_mut().flush();
    waiting
}

#[test]
fn a_tool_is_called_only_while_its_plugin_is_allowed_to_give_it() {
    let scratch = Scratch::new("tools_gate");
    let mut app =
        home::app(&scratch, &[probe().asking(&["agent.tool"])]);
    app.update();

    let mut waiting = call(&mut app);

    let answer = waiting.take().unwrap().unwrap_err();
    assert!(answer.contains("not giving tools"), "{answer}");
}

#[test]
fn a_call_that_the_plugin_does_not_answer_ends_with_an_error() {
    let scratch = Scratch::new("tools_silent");
    let allowed = probe().asking(&["agent.tool"]);
    let mut app =
        home::app(&scratch, &[allowed.granted(&["agent.tool"])]);
    app.world_mut().resource_mut::<Limits>().call_frames = 3;
    app.update();

    let mut waiting = call(&mut app);
    assert!(waiting.take().is_none());
    (0..6).for_each(|_| app.update());

    let answer = waiting.take().unwrap().unwrap_err();
    assert!(answer.contains("did not answer"), "{answer}");
}

#[test]
fn a_plugin_that_gives_tools_may_not_act_as_the_chief() {
    let asks = |capability: &str| {
        Manifest::parse(&format!(
            "id = \"p\"\nname = \"P\"\nversion = \
             \"0.1.0\"\n[[asks]]\ncapability = \
             \"agent.tool\"\nreason = \"x\"\n[[asks]]\ncapability = \
             {capability:?}\nreason = \"y\"\n"
        ))
        .unwrap()
        .asked()
    };

    assert!(asks("db").is_ok());
    for with in ["chat.send", "commands.run", "plugin.use"] {
        assert!(
            matches!(asks(with), Err(Refusal::Together(..))),
            "{with}"
        );
    }
}
