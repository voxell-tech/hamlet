//! The Inspector: whoever is selected, or every hamlet when nobody
//! is.

mod agent;
mod chief;
mod folder;
mod gone;
pub mod history;
mod house;
mod lists;
pub mod overview;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{MessageWriter, Messages};
use bevy::ecs::schedule::common_conditions::{
    resource_changed, resource_exists, resource_exists_and_changed,
};
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Commands, Local, Res};
use bevy::ecs::world::World;
use bevy_fynix::{ViewExt, derived, every_frame, keyed};
use hamlet_agents_api::agents::Left;
use hamlet_agents_api::convo::of_agent;
use hamlet_agents_api::papers::keep_papers;
use hamlet_agents_api::{roll, words};
use hamlet_chat::me::{Me, talk_to, talk_to_mayor};
use hamlet_chat::{chat, deed, links, markdown, me};
use hamlet_limits::{models, rules};
use hamlet_realm_api::pick::{Selected, Thing};
use hamlet_realm_api::sound::{PanelCue, PanelSound, write};
use hamlet_sdk::command::{Subject, kind};
use hamlet_sdk::dock::{lit_panel, show_tab};
use hamlet_sdk::feel::Lit;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::inspector::InspectorSections;
use hamlet_sdk::looks::filling;
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};
use hamlet_sdk::{
    AnyView, Cx, View, dock, feel, icons, looks, platform, tabs,
    theme, with_cx,
};

use self::agent::show_prompt;
pub use self::agent::{Briefing, Section};
use self::house::house;

/// Adds the Inspector plugin.
pub fn plugin(app: &mut App) {
    app.add_plugins((
        overview::plugin,
        history::plugin,
        folder::plugin,
    ))
    .init_resource::<Section>()
    .init_resource::<Briefing>()
    .add_systems(
        Update,
        (
            tabs::mark::<Section>,
            agent::leave_prompt
                .run_if(resource_exists_and_changed::<Me>),
            agent::leave_gone.run_if(
                resource_changed::<InspectorSections>
                    .or_eager(resource_changed::<Papers>),
            ),
            keep_papers.run_if(resource_changed::<Section>),
            come_forward
                .run_if(resource_exists_and_changed::<Selected>),
            tab_sound.run_if(resource_exists::<Messages<PanelCue>>),
        ),
    )
    .add_hamlet_plugin::<InspectorPlugin>();
}

/// The plugin that adds the Inspector.
pub struct InspectorPlugin;

impl HamletPlugin for InspectorPlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("inspector").named("Inspector")
    }

    fn entry(entry: &mut Entry) {
        entry.panel("agent", "INSPECTOR", Glyph::Lens, || {
            lit_panel(
                screen(),
                Lit {
                    alarm: false,
                    flash: true,
                },
            )
        });
        entry.command_with("prompt", "Show the Prompt", prompt);
        for target in [kind::AGENT, kind::HOUSE, kind::MAYOR] {
            entry.context_item(target, 20, "Prompt", "prompt", None);
        }
    }
}

/// The subject of the panel: every hamlet when nothing is selected, a
/// hamlet whose common is, an agent that is gone, a chief, or the
/// agent the chat is with.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Showing {
    Overview,
    Hamlet(Entity),
    Gone(Entity),
    Chief(Entity),
    /// The library's shelves.
    Library,
    Agent,
}

fn showing(world: &World) -> Showing {
    let selected = world.get_resource::<Selected>().and_then(|s| s.0);
    match selected {
        None => Showing::Overview,
        Some(Thing::Library) => Showing::Library,
        Some(Thing::Common(hamlet))
            if world.get_entity(hamlet).is_ok() =>
        {
            Showing::Hamlet(hamlet)
        }
        // One that came back is shown as itself again.
        Some(Thing::Ruin(agent))
            if world.get::<Left>(agent).is_some() =>
        {
            Showing::Gone(agent)
        }
        Some(Thing::Chief(chief) | Thing::ChiefPlace(chief))
            if world.get_entity(chief).is_ok() =>
        {
            Showing::Chief(chief)
        }
        Some(
            Thing::Common(_)
            | Thing::Ruin(_)
            | Thing::Chief(_)
            | Thing::ChiefPlace(_)
            | Thing::Clockwork(_)
            | Thing::Workbench(_),
        ) => Showing::Overview,
        Some(_) => Showing::Agent,
    }
}

/// The Inspector's contents.
pub fn screen() -> impl View {
    keyed(derived(showing).when(every_frame()), |showing| {
        match showing {
            Showing::Overview => overview::overview(),
            Showing::Hamlet(hamlet) => overview::hamlet(*hamlet),
            Showing::Gone(agent) => gone::ruin(*agent),
            Showing::Chief(chief) => chief::panel(*chief),
            Showing::Library => gone::library().boxed(),
            Showing::Agent => agent::living().boxed(),
        }
    })
    .within(filling())
}

/// A click in the world brings the Inspector to the front.
pub fn come_forward(world: &mut World) {
    show_tab(world, "agent");
}

/// Turns the page to the prompt of the agent a menu was opened on,
/// and points the chat at it.
fn prompt(commands: &mut Commands, subject: &Subject) {
    let subject = subject.clone();
    commands.queue(move |world: &mut World| {
        let agent =
            subject.id.parse().ok().and_then(Entity::try_from_bits);
        match (subject.kind == kind::MAYOR, agent) {
            (true, _) => talk_to_mayor(world),
            (_, Some(agent)) => talk_to(world, agent),
            _ => {}
        }
        show_prompt(world);
    });
}

/// A tab of the agent's page chosen by hand.
fn tab_sound(
    section: Res<Section>,
    mut was: Local<Option<Section>>,
    mut turn: Local<u32>,
    mut cues: MessageWriter<PanelCue>,
) {
    let before = was.replace(*section);
    if before.is_some_and(|before| before != *section) {
        write(&mut cues, &mut turn, PanelSound::Tab);
    }
}
