//! The inputs that wake the server: the channel they arrive on, where
//! each begins, and how a system is added to take one in.

use std::sync::Mutex;

use bevy_app::{App, First, Update};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::Message;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::system::{ResMut, ScheduleSystem};
use bevy_ecs::world::World;
use hamlet_protocol::client::RequestId;
use tokio::sync::mpsc;

use super::failure;
use crate::realm::Realm;

/// The one channel inputs arrive on, the error handler, where each
/// input begins, and who is told when one fails.
pub(crate) fn plugin(app: &mut App) {
    let (sender, receiver) = mpsc::unbounded_channel();
    app.set_error_handler(failure::record)
        .insert_resource(InputSender(sender))
        .insert_resource(InputReceiver(Mutex::new(receiver)))
        .init_resource::<InputOrigin>()
        .add_systems(First, begin_input);
}

/// An input that wakes the server: the message it writes, for the
/// systems of its kind to read.
pub(crate) struct Input(Box<dyn FnOnce(&mut World) + Send>);

impl Input {
    pub fn of(message: impl Message) -> Self {
        Self(Box::new(move |world: &mut World| {
            world.write_message(message);
        }))
    }

    /// Writes the input's message into `world`.
    pub(super) fn write(self, world: &mut World) {
        (self.0)(world);
    }
}

/// Where everything that wakes the server sends its input: a
/// runtime's output, the jobs runner, a connection, a task, the
/// folder of the tools added by hand, a terminal, the listener.
#[derive(Resource, Clone)]
pub(crate) struct InputSender(mpsc::UnboundedSender<Input>);

/// The runner stopped taking inputs: the server is stopping.
#[derive(Debug)]
pub(crate) struct RunnerGone;

impl InputSender {
    /// Sends `message` to the runner as an input of its own.
    pub fn send(
        &self,
        message: impl Message,
    ) -> Result<(), RunnerGone> {
        self.0.send(Input::of(message)).map_err(|_| RunnerGone)
    }
}

/// The receiving end of the inputs, until the runner takes it.
#[derive(Resource)]
pub(super) struct InputReceiver(
    pub(super) Mutex<mpsc::UnboundedReceiver<Input>>,
);

/// Who sent the input being run, so a failure can tell them.
#[derive(Resource, Default, Clone)]
pub(crate) enum InputOrigin {
    /// The server itself: a timer, a task, the jobs runner.
    #[default]
    Server,
    /// A client's request, answered with the failure.
    Client {
        client: u64,
        request: Option<RequestId>,
    },
    /// A runtime's message, answered with the failure when the
    /// runtime awaits an answer to `awaited`.
    Runtime {
        runtime: Entity,
        awaited: Option<String>,
    },
}

/// Begins the input's transaction, which `PostUpdate` keeps or rolls
/// back.
fn begin_input(
    mut realm: ResMut<Realm>,
    mut origin: ResMut<InputOrigin>,
) -> bevy_ecs::error::Result {
    *origin = InputOrigin::Server;
    hamlet_store::begin(realm.store().connection())?;
    Ok(())
}

/// Adds the systems that take in inputs.
pub(crate) trait AddHandler {
    /// Adds `handler` to `Update` for the input message `T`, run only
    /// when an input brought one; it drains every one it brought.
    fn add_handler<T: Message, M>(
        &mut self,
        handler: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self;
}

impl AddHandler for App {
    fn add_handler<T: Message, M>(
        &mut self,
        handler: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        self.add_message::<T>()
            .add_systems(Update, handler.run_if(on_message::<T>))
    }
}
