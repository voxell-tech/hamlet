//! The requests of plugins: each checked against what the person
//! allowed, asked about on its first use, and answered, and the
//! changes sent to the plugins that watch.

use bevy::app::{App, PostUpdate};
use bevy::ecs::change_detection::Mut;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Local, Query, Res, ResMut};
use hamlet_engine_api::agents::LOCAL;
use hamlet_engine_api::desktop::DesktopSettings;
use hamlet_engine_api::{
    DesktopEvent, DesktopReply, DesktopRequest, Failure, Message,
    Reply, Request, Ticket,
};
use hamlet_engine_core::{Home, LogLine};
use hamlet_engine_grants::{
    Decided, FirstUse, Grants, Verdict, Waiting,
};
use hamlet_engine_plugins::{Asked, Deliver, Disable, PluginInfo};

use crate::awake::KeepAwake;
use crate::open::{Opened, opening};

/// The most characters of an address under the pointer that are
/// kept.
const LONGEST: usize = 2048;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Watchers>()
        .init_resource::<Hovered>()
        .add_observer(switched_off)
        .add_systems(PostUpdate, take);
}

/// The plugins that watch, each by the ticket of its `Watch`.
#[derive(Resource, Default)]
struct Watchers(Vec<(Entity, Ticket)>);

/// The address under the pointer, and the plugin that told of it.
#[derive(Resource, Default)]
struct Hovered(Option<(Entity, String)>);

/// What the service needs to answer a request that may go on, and
/// what it has to tell.
struct Answering<'a, 'w, 's> {
    plugins: &'a Query<'w, 's, &'static PluginInfo>,
    home: &'a Home,
    keep: Mut<'a, KeepAwake>,
    opened: Mut<'a, Opened>,
    watchers: Mut<'a, Watchers>,
    hovered: Mut<'a, Hovered>,
    told: Vec<DesktopEvent>,
    log: Vec<LogLine>,
}

/// Takes the desktop requests that may go on and answers them, asks
/// the person about those used for the first time, refuses the rest,
/// and tells the watchers what changed.
fn take(
    mut asked: MessageReader<Asked>,
    mut decided: MessageReader<Decided>,
    plugins: Query<&'static PluginInfo>,
    grants: Res<Grants>,
    home: Res<Home>,
    mut keep: ResMut<KeepAwake>,
    mut opened: ResMut<Opened>,
    mut watchers: ResMut<Watchers>,
    mut hovered: ResMut<Hovered>,
    mut parked: Local<Waiting<(Asked, String)>>,
    mut deliver: MessageWriter<Deliver>,
    mut first_use: MessageWriter<FirstUse>,
    mut log: MessageWriter<LogLine>,
) {
    parked.keep(|plugin| plugins.contains(plugin));
    // Each request that may go on, with the target it was allowed.
    let mut ready = Vec::new();
    for ((waiting, target), allowed) in parked.settle(decided.read())
    {
        if allowed {
            ready.push((waiting, Some(target)));
        } else if let Request::Desktop(request) = &waiting.request
            && let Some(capability) = request.capability()
        {
            deliver.write(waiting.reply(Err(Failure::Refused(
                format!("{} was not allowed", capability.name()),
            ))));
        }
    }
    for request in asked.read() {
        let Request::Desktop(desktop) = &request.request else {
            continue;
        };
        let Ok(plugin) = plugins.get(request.plugin) else {
            continue;
        };
        let Some(capability) = desktop.capability() else {
            ready.push((request.clone(), None));
            continue;
        };
        let target = match desktop {
            DesktopRequest::Open(written) => {
                match opening(&plugin.asks, written) {
                    Ok((_, target)) => target,
                    Err(why) => {
                        deliver.write(request.reply(Err(why)));
                        continue;
                    }
                }
            }
            _ => LOCAL.to_owned(),
        };
        match grants.verdict(plugin, capability, &target) {
            Verdict::Allowed => {
                ready.push((request.clone(), Some(target)));
            }
            Verdict::Asking => parked.park(
                request.plugin,
                capability,
                target.clone(),
                (request.clone(), target),
                &mut first_use,
            ),
            Verdict::Refused => {
                deliver.write(request.reply(Err(Failure::Refused(
                    format!(
                        "{} {target} is not allowed",
                        capability.name()
                    ),
                ))));
            }
        }
    }
    let mut answering = Answering {
        plugins: &plugins,
        home: &home,
        keep: keep.reborrow(),
        opened: opened.reborrow(),
        watchers: watchers.reborrow(),
        hovered: hovered.reborrow(),
        told: Vec::new(),
        log: Vec::new(),
    };
    answering.forget_gone();
    for (request, allowed) in ready {
        let Request::Desktop(desktop) = &request.request else {
            continue;
        };
        let reply = answering.answer(
            &request,
            desktop.clone(),
            allowed.as_deref(),
        );
        deliver.write(request.reply(reply.map(Reply::Desktop)));
    }
    for line in answering.log.drain(..) {
        log.write(line);
    }
    for event in answering.told {
        answering.watchers.tell(&event, &mut deliver);
    }
}

/// Stops telling a plugin switched off, which watches again once it
/// is on, and lets go of the address it told of.
fn switched_off(
    off: On<Disable>,
    mut watchers: ResMut<Watchers>,
    mut hovered: ResMut<Hovered>,
    mut deliver: MessageWriter<Deliver>,
) {
    watchers.0.retain(|(plugin, _)| *plugin != off.entity);
    if hovered
        .0
        .as_ref()
        .is_some_and(|(plugin, _)| *plugin == off.entity)
    {
        hovered.0 = None;
        watchers.tell(&DesktopEvent::Hovered(None), &mut deliver);
    }
}

impl Watchers {
    /// Sends `event` to each watcher.
    fn tell(
        &self,
        event: &DesktopEvent,
        deliver: &mut MessageWriter<Deliver>,
    ) {
        for (plugin, ticket) in &self.0 {
            deliver.write(Deliver {
                plugin: *plugin,
                message: Message::Reply {
                    ticket: *ticket,
                    reply: Ok(Reply::Desktop(DesktopReply::Event(
                        event.clone(),
                    ))),
                },
                depth: 1,
            });
        }
    }
}

impl Answering<'_, '_, '_> {
    /// Lets go of the watchers that are gone, and of the address
    /// under the pointer when the plugin that told of it is.
    fn forget_gone(&mut self) {
        let plugins = self.plugins;
        if self
            .watchers
            .0
            .iter()
            .any(|(plugin, _)| !plugins.contains(*plugin))
        {
            self.watchers
                .0
                .retain(|(plugin, _)| plugins.contains(*plugin));
        }
        if let Some((plugin, _)) = &self.hovered.0
            && !plugins.contains(*plugin)
        {
            self.hovered.0 = None;
            self.told.push(DesktopEvent::Hovered(None));
        }
    }

    fn settings(&self) -> DesktopSettings {
        DesktopSettings {
            keep_awake: self.keep.0,
        }
    }

    /// What `request` of the plugin `asked`, which may go on on the
    /// target `allowed`, comes to.
    fn answer(
        &mut self,
        asked: &Asked,
        request: DesktopRequest,
        allowed: Option<&str>,
    ) -> Result<DesktopReply, Failure> {
        match request {
            DesktopRequest::Watch => {
                self.watchers
                    .0
                    .retain(|(plugin, _)| *plugin != asked.plugin);
                self.watchers.0.push((asked.plugin, asked.ticket));
                Ok(DesktopReply::Watching)
            }
            DesktopRequest::Settings => {
                Ok(DesktopReply::Settings(self.settings()))
            }
            DesktopRequest::SetKeepAwake(on) => {
                if self.keep.0 != on {
                    self.keep.0 = on;
                    self.told.push(DesktopEvent::Settings(
                        self.settings(),
                    ));
                }
                self.keep.save(self.home).map_err(Failure::Failed)?;
                Ok(DesktopReply::Settings(self.settings()))
            }
            DesktopRequest::Open(written) => {
                let plugin = self
                    .plugins
                    .get(asked.plugin)
                    .map_err(|_| Failure::NotFound)?;
                let (opening, target) =
                    opening(&plugin.asks, &written)?;
                // A link changed while the person was asked may put
                // the path under another folder.
                if allowed != Some(target.as_str()) {
                    return Err(Failure::Refused(format!(
                        "{written:?} is no longer under {}",
                        allowed.unwrap_or_default()
                    )));
                }
                let child = opening.start().map_err(|why| {
                    Failure::Failed(format!(
                        "nothing could open it: {why}"
                    ))
                })?;
                self.opened.0.push(child);
                self.log.push(LogLine::info(
                    plugin.id.as_str(),
                    format!("opens {written} under {target}"),
                ));
                Ok(DesktopReply::Done)
            }
            DesktopRequest::Hover(address) => {
                let now = match address {
                    Some(address) => {
                        if address.chars().any(char::is_control) {
                            return Err(Failure::Refused(
                                "an address is one line".into(),
                            ));
                        }
                        let address =
                            address.chars().take(LONGEST).collect();
                        Some((asked.plugin, address))
                    }
                    // Only the plugin that told of an address takes
                    // it away.
                    None => self.hovered.0.clone().filter(
                        |(plugin, _)| *plugin != asked.plugin,
                    ),
                };
                if self.hovered.0 != now {
                    self.told.push(DesktopEvent::Hovered(
                        now.as_ref()
                            .map(|(_, address)| address.clone()),
                    ));
                    self.hovered.0 = now;
                }
                Ok(DesktopReply::Done)
            }
        }
    }
}
