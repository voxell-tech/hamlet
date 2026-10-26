//! The pages of a Hamlet account: signing up, its code, its
//! username, signing in, a forgotten password, and changing the
//! username or the password. First-run setup, the Agent Bazaar and
//! Settings each show them under their own head; what they send is
//! in `bazaar/market/account.rs`.

use bevy::app::{App, PostUpdate};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Res;
use bevy::ecs::world::World;
use bevy::input_focus::{InputFocus, InputFocusSystems};
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, label, row, text_field};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    bare, buttons, close, description, good_action, group, note,
    quiet, section, small_rules, text_input,
};
use hamlet_ui::widgets::icons::Glyph;

use crate::bazaar::market::{
    change_password, choose_username, forgot, resend, reset, sign_in,
    sign_up, verify,
};
use crate::secret::secret_field;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Form>()
        .init_resource::<Committing>()
        // After Bevy tells a field it lost focus and fynix hands its
        // text to the form, so a button sends what was typed in the
        // frame it was pressed.
        .add_systems(
            PostUpdate,
            run_committed
                .run_if(|pending: Res<Committing>| {
                    pending.0.is_some()
                })
                .after(InputFocusSystems::FocusChangeEvents),
        );
}

/// A page of a Hamlet account.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    /// No page is open.
    #[default]
    Shut,
    SignIn,
    SignUp,
    /// The code emailed to verify the email.
    Code,
    /// The username chosen once the email is verified.
    Username,
    /// The email a code is sent to, to choose a new password with.
    Forgot,
    /// That code and the new password.
    Reset,
    Rename,
    Password,
}

impl Page {
    /// The page's name, at its head.
    pub fn title(self) -> &'static str {
        match self {
            Page::Shut => "",
            Page::SignIn => "SIGN IN",
            Page::SignUp => "CREATE A HAMLET ACCOUNT",
            Page::Code => "CHECK YOUR EMAIL",
            Page::Username => "CHOOSE A USERNAME",
            Page::Forgot => "FORGOT PASSWORD",
            Page::Reset => "CHOOSE A NEW PASSWORD",
            Page::Rename => "CHANGE USERNAME",
            Page::Password => "CHANGE PASSWORD",
        }
    }
}

/// Where the account's pages are shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Place {
    #[default]
    Welcome,
    Bazaar,
    Settings,
}

/// The page of a Hamlet account open, where, and what is typed in it.
#[derive(Resource, Clone, Default, PartialEq, Debug)]
pub struct Form {
    pub page: Page,
    pub at: Place,
    pub email: String,
    pub password: String,
    /// The password typed again.
    pub again: String,
    pub code: String,
    pub username: String,
    /// The new password, when it is changed.
    pub fresh: String,
    /// The bazaar's server is asked.
    pub asking: bool,
    /// Why the last thing asked did not work.
    pub problem: Option<String>,
    /// What came of the last thing asked, when it worked.
    pub said: Option<String>,
}

impl Form {
    /// The page open at `place`.
    pub fn at(&self, place: Place) -> Page {
        match self.at == place {
            true => self.page,
            false => Page::Shut,
        }
    }
}

/// Opens `page` at `place`, keeping the email typed.
pub(crate) fn open(world: &mut World, place: Place, page: Page) {
    let mut form = world.resource_mut::<Form>();
    let email = std::mem::take(&mut form.email);
    *form = Form {
        page,
        at: place,
        email,
        ..Form::default()
    };
}

/// Turns to `page` where the form is, telling `said`.
pub(crate) fn turn(
    world: &mut World,
    page: Page,
    said: Option<String>,
) {
    let mut form = world.resource_mut::<Form>();
    form.page = page;
    form.asking = false;
    form.problem = None;
    form.said = said;
    form.code.clear();
}

/// Shuts the account's pages and forgets what was typed in them.
pub(crate) fn shut(world: &mut World) {
    let at = world.resource::<Form>().at;
    *world.resource_mut::<Form>() = Form {
        at,
        ..Form::default()
    };
}

/// A button's act, waiting for the fields to hand over their text.
#[derive(Resource, Default)]
pub(crate) struct Committing(Option<fn(&mut World)>);

fn run_committed(world: &mut World) {
    if let Some(act) = world.resource_mut::<Committing>().0.take() {
        act(world);
    }
}

/// Runs `act` once the field typed in has handed over its text.
pub(crate) fn commit(world: &mut World, act: fn(&mut World)) {
    if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
        focus.clear();
    }
    world.resource_mut::<Committing>().0 = Some(act);
}

/// What goes in a field of the form.
#[derive(Clone, Copy)]
enum Field {
    Email,
    Password,
    Again,
    Code,
    Username,
    Fresh,
}

impl Field {
    fn of(self, form: &mut Form) -> &mut String {
        match self {
            Field::Email => &mut form.email,
            Field::Password => &mut form.password,
            Field::Again => &mut form.again,
            Field::Code => &mut form.code,
            Field::Username => &mut form.username,
            Field::Fresh => &mut form.fresh,
        }
    }
}

/// A field of the form under its `name`, its words drawn as stars
/// when it is `secret`.
fn field(name: &'static str, which: Field, secret: bool) -> AnyView {
    let value = resource::<Form, _>(move |form| {
        which.of(&mut form.clone()).clone()
    });
    let typed = move |world: &mut World, text: String| {
        *which.of(&mut world.resource_mut::<Form>()) = text;
    };
    let control = match secret {
        true => secret_field(value, typed).boxed(),
        false => text_field(value, typed).rules(text_input).boxed(),
    };
    close((section(name), control)).boxed()
}

/// How the last thing asked went: that it is asked, why it did not
/// work, in red, or what came of it.
fn news(waiting: &'static str) -> AnyView {
    keyed(
        resource::<Form, _>(|form| {
            (form.asking, form.problem.clone(), form.said.clone())
        }),
        move |(asking, problem, said)| match (problem, said) {
            _ if *asking => description(waiting).boxed(),
            (Some(why), _) => label(why.clone())
                .tone(Tone::Critical)
                .rules(small_rules)
                .boxed(),
            (None, Some(said)) => description(said.clone()).boxed(),
            (None, None) => bare().boxed(),
        },
    )
    .within(bare())
    .boxed()
}

/// The ways from one page to another, under its button.
fn ways(ways: Vec<AnyView>) -> AnyView {
    row(ways).gap(12.0).align(AlignItems::Center).boxed()
}

/// The words, fields and buttons of `page` at `place`.
pub(crate) fn body(page: Page, place: Place) -> AnyView {
    let to = move |page: Page| {
        move |world: &mut World| open(world, place, page)
    };
    let parts = match page {
        Page::Shut => return bare().boxed(),
        Page::SignIn => vec![
            description(
                "Sign in with the email and password of your Hamlet \
                 account.",
            )
            .boxed(),
            field("EMAIL", Field::Email, false),
            field("PASSWORD", Field::Password, true),
            news("Signing in..."),
            buttons(vec![good_action(
                Glyph::Signpost,
                "SIGN IN",
                |world| {
                    commit(world, sign_in);
                },
            )])
            .boxed(),
            ways(vec![
                quiet(
                    Glyph::Restore,
                    "FORGOT PASSWORD",
                    to(Page::Forgot),
                ),
                quiet(
                    Glyph::Plus,
                    "CREATE AN ACCOUNT",
                    to(Page::SignUp),
                ),
            ]),
        ],
        Page::SignUp => vec![
            description(
                "A Hamlet account is needed for the Agent Bazaar, \
                 and keeps your OpenRouter key on every computer \
                 you sign in on. We email you a code to check it is \
                 yours.",
            )
            .boxed(),
            field("EMAIL", Field::Email, false),
            field("USERNAME", Field::Username, false),
            note(
                "Shown on what you sell and review: 3 to 24 \
                 lowercase letters, digits, - or _. You can change \
                 it later.",
            )
            .boxed(),
            field("PASSWORD", Field::Password, true),
            field("PASSWORD AGAIN", Field::Again, true),
            note("At least 10 characters.").boxed(),
            news("Creating your account..."),
            buttons(vec![good_action(
                Glyph::Plus,
                "CREATE ACCOUNT",
                |world| commit(world, sign_up),
            )])
            .boxed(),
            ways(vec![quiet(
                Glyph::Signpost,
                "I HAVE AN ACCOUNT",
                to(Page::SignIn),
            )]),
        ],
        Page::Code => vec![
            description(
                "Type the 6-digit code we emailed you. It lasts a \
                 few minutes.",
            )
            .boxed(),
            field("CODE", Field::Code, false),
            news("Checking the code..."),
            buttons(vec![
                good_action(Glyph::Check, "CONTINUE", |world| {
                    commit(world, verify);
                }),
                quiet(Glyph::Restore, "SEND AGAIN", resend),
            ])
            .boxed(),
        ],
        Page::Username | Page::Rename => vec![
            description(
                "Your username is shown on what you sell and \
                 review. Use 3 to 24 lowercase letters, digits, - \
                 or _. You can change it later.",
            )
            .boxed(),
            field("USERNAME", Field::Username, false),
            news("Saving your username..."),
            buttons(vec![good_action(
                Glyph::Check,
                "SAVE",
                |world| {
                    commit(world, choose_username);
                },
            )])
            .boxed(),
        ],
        Page::Forgot => vec![
            description(
                "We email you a code to choose a new password with. \
                 The OpenRouter key kept with your account is lost, \
                 as it was locked with the old password: connect \
                 OpenRouter again afterwards.",
            )
            .boxed(),
            field("EMAIL", Field::Email, false),
            news("Sending a code..."),
            buttons(vec![good_action(
                Glyph::Signpost,
                "SEND A CODE",
                |world| {
                    commit(world, forgot);
                },
            )])
            .boxed(),
            ways(vec![quiet(
                Glyph::Back,
                "SIGN IN",
                to(Page::SignIn),
            )]),
        ],
        Page::Reset => vec![
            description(
                "Type the code we emailed you, and a new password.",
            )
            .boxed(),
            field("CODE", Field::Code, false),
            field("NEW PASSWORD", Field::Fresh, true),
            field("NEW PASSWORD AGAIN", Field::Again, true),
            note("At least 10 characters.").boxed(),
            news("Changing your password..."),
            buttons(vec![good_action(
                Glyph::Check,
                "CHANGE PASSWORD",
                |world| {
                    commit(world, reset);
                },
            )])
            .boxed(),
        ],
        Page::Password => vec![
            description(
                "Your other computers are signed out, and sign in \
                 again with the new password.",
            )
            .boxed(),
            field("PASSWORD", Field::Password, true),
            field("NEW PASSWORD", Field::Fresh, true),
            field("NEW PASSWORD AGAIN", Field::Again, true),
            note("At least 10 characters.").boxed(),
            news("Changing your password..."),
            buttons(vec![good_action(
                Glyph::Check,
                "CHANGE PASSWORD",
                |world| {
                    commit(world, change_password);
                },
            )])
            .boxed(),
        ],
    };
    group(parts).boxed()
}
