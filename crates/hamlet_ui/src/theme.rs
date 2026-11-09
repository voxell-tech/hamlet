//! The theme of the panels (docs/ui-warmth.md): one variant's
//! palette, from a family under `assets/themes/`, in the measures of
//! `assets/measures.toml`. Nothing of the panels is built until a
//! theme is shown.

mod base;
pub mod contrast;
mod derive;
mod themes;

use std::ops::Deref;
use std::time::Duration;

use bevy::app::{App, PostUpdate};
use bevy::asset::Asset;
use bevy::color::{Color, Luminance, Mix, Srgba};
use bevy::ecs::message::{Message, Messages};
use bevy::ecs::query::{Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{If, Query, Res, SystemParam};
use bevy::ecs::world::World;
use bevy::prelude::ClearColor;
use bevy::reflect::TypePath;
use bevy::state::app::AppExtStates;
use bevy::state::state::{NextState, State, States};
use bevy::text::{FontSource, FontWeight, TextColor, TextFont};
use bevy::ui::{BoxShadow, UiSystems, px};
use bevy::window::WindowTheme;
use bevy_fynix::tokens::{
    Curve, Motion, MotionTokens, SpacingTokens, SurfaceTokens,
    TextTokens, Tone,
};
use hamlet_assets::{File, Tuning, TuningPlugin, tint};
use serde::{Deserialize, Serialize};

pub use self::base::{Base, Hue, Mode};
use self::contrast::Part;
pub use self::themes::{Theme, ThemeId, Themes, Variant, Variants};
use crate::widgets::platform::Display;

pub fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<Measures>::default())
        .insert_resource(Measures::designed())
        .insert_resource(Themes::shipped())
        .add_message::<ThemeChanged>()
        .init_state::<ThemeState>()
        .add_plugins((themes::plugin, weight_plugin));
}

/// Sets the weight of regular words by their ink; also for a window
/// that shows its theme by hand.
pub fn weight_plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        weigh_words.before(UiSystems::Content),
    );
}

/// Sets regular words in dark ink at [`TextSizes::dark_ink_weight`],
/// and back to regular in light ink. Glyph edges blend in linear
/// light, which thins dark letters on a light ground as much as it
/// thickens light ones on a dark ground. Bold words and the display
/// face keep their weight.
fn weigh_words(
    theme: Themed,
    display: Option<Res<Display>>,
    mut words: Query<
        (&mut TextFont, &TextColor),
        Or<(Changed<TextFont>, Changed<TextColor>)>,
    >,
) {
    let heavier = FontWeight(theme.text.dark_ink_weight);
    for (mut font, ink) in &mut words {
        let display_face = match (&font.font, &display) {
            (FontSource::Handle(face), Some(display)) => {
                *face == display.0
            }
            _ => false,
        };
        // Anything below bold, so a weight tuned while the window
        // runs moves the words set at the one before.
        let plain = (FontWeight::NORMAL.0..FontWeight::BOLD.0)
            .contains(&font.weight.0);
        if display_face || !plain {
            continue;
        }
        // Dark ink reads better on white than on black.
        let dark = contrast::contrast(ink.0, Color::WHITE)
            > contrast::contrast(ink.0, Color::BLACK);
        let weight = match dark {
            true => heavier,
            false => FontWeight::NORMAL,
        };
        if font.weight != weight {
            font.weight = weight;
        }
    }
}

/// The theme, for a system: one that takes it waits for the theme to
/// load.
#[derive(SystemParam)]
pub struct Themed<'w> {
    theme: If<Res<'w, bevy_fynix::Theme<HamletTheme>>>,
}

impl Deref for Themed<'_> {
    type Target = HamletTheme;

    fn deref(&self) -> &HamletTheme {
        &self.theme.0.0
    }
}

/// Whether a theme has been shown. The panels are built on
/// [`ThemeState::Ready`], which [`show`] sets the first time.
#[derive(
    States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default,
)]
pub enum ThemeState {
    #[default]
    Loading,
    Ready,
}

/// The theme the panels are drawn in: one variant's palette in the
/// shared measures.
#[derive(Clone, PartialEq, Debug)]
pub struct HamletTheme {
    pub palette: Palette,
    pub text: TextSizes,
    pub space: Spacing,
    pub shadow: Fall,
    pub motion: Timing,
}

/// `assets/measures.toml`: everything of the panels that is not a
/// colour.
#[derive(
    Asset, TypePath, Resource, Clone, PartialEq, Debug, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct Measures {
    pub text: TextSizes,
    pub space: Spacing,
    pub shadow: Fall,
    pub motion: Timing,
}

impl Tuning for Measures {
    const FILE: File = hamlet_assets::MEASURES;

    fn apply(&self, was: &Self, world: &mut World) {
        hamlet_assets::follow(world, was, self);
    }
}

/// Every colour of one variant, as [`Palette::derive`] makes them
/// from the fifteen colours of a theme file.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct Palette {
    /// A panel's background, the darkest surface but for a text
    /// input.
    #[serde(with = "tint")]
    pub surface: Color,
    /// Text that stands back.
    #[serde(with = "tint")]
    pub grey_1: Color,
    /// A button's fill, and its fill under the pointer.
    #[serde(with = "tint")]
    pub button: Color,
    #[serde(with = "tint")]
    pub button_hover: Color,
    /// The line between two surfaces.
    #[serde(with = "tint")]
    pub border: Color,
    /// The fill of what is selected in a list, a step off the panel.
    #[serde(with = "tint")]
    pub selected: Color,
    /// The colour of text that is read.
    #[serde(with = "tint")]
    pub gold_1: Color,
    /// Gold, from the text of a button down to a hairline.
    #[serde(with = "tint")]
    pub gold_2: Color,
    #[serde(with = "tint")]
    pub gold_3: Color,
    #[serde(with = "tint")]
    pub gold_4: Color,
    #[serde(with = "tint")]
    pub gold_5: Color,
    #[serde(with = "tint")]
    pub gold_6: Color,
    /// A button's fill under the pointer.
    #[serde(with = "tint")]
    pub gold_hover: Color,
    /// Text and icons that are live, wait or can be followed, a
    /// border under the pointer, and the ring of what has focus.
    #[serde(with = "tint")]
    pub accent: Color,
    /// The colour of light: a control's warmth, the lamp over a
    /// panel, the underglow, the cursor's lamp. Never read as text.
    #[serde(with = "tint")]
    pub glow: Color,
    /// The bright centre of a glow.
    #[serde(with = "tint")]
    pub glow_core: Color,
    /// A particle that rises from what was just done.
    #[serde(with = "tint")]
    pub particle: Color,
    /// The fill of a primary button, away from its glow and near
    /// it.
    #[serde(with = "tint")]
    pub primary_low: Color,
    #[serde(with = "tint")]
    pub primary_high: Color,
    /// The agents' own colour, as the world draws it.
    #[serde(with = "tint")]
    pub agent: Color,
    /// A card in a list: a shade above the panel, and no more.
    #[serde(with = "tint")]
    pub card: Color,
    /// The fill of a text input, a shade off the panel.
    #[serde(with = "tint")]
    pub input: Color,
    /// The background under code in a reply.
    #[serde(with = "tint")]
    pub code: Color,
    /// The highlight behind words selected in a message.
    #[serde(with = "tint")]
    pub text_selection: Color,
    /// The glow that gathers at the bottom of a surface.
    #[serde(with = "tint")]
    pub bottom_glow: Color,
    /// Red, for what is refused, stopped or at fault.
    #[serde(with = "tint")]
    pub red: Color,
    /// A danger button's fill.
    #[serde(with = "tint")]
    pub danger: Color,
    /// The border of a danger button at rest.
    #[serde(with = "tint")]
    pub danger_border: Color,
    /// A danger button's fill under the pointer.
    #[serde(with = "tint")]
    pub danger_hover: Color,
    /// The glow of a danger button.
    #[serde(with = "tint")]
    pub danger_glow: Color,
    /// Green, for a line put in a file.
    #[serde(with = "tint")]
    pub green: Color,
    /// The background under a line put in a file.
    #[serde(with = "tint")]
    pub green_deep: Color,
    /// The shadow under what floats, with its alpha.
    #[serde(with = "tint")]
    pub shadow: Color,
    /// What each status is drawn in.
    #[serde(skip)]
    pub status: Statuses,
}

/// How one status is drawn: its mark for icons, dots and bars, its ink
/// for words, a tinted fill with the ink to use on it, and its solid
/// form for a pill, a circle or a button, with the colour of what is
/// on it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Status {
    /// The hue kept as far as a shape needs only its contrast of a
    /// mark.
    pub mark: Color,
    /// The hue moved until words of it read.
    pub ink: Color,
    /// A plate under the ink: pale in light, deep in dark.
    pub fill: Color,
    /// The plate under the pointer.
    pub fill_hover: Color,
    pub on_fill: Color,
    pub solid: Color,
    pub on_solid: Color,
}

/// What a status means, and so which hue draws it everywhere.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusKind {
    /// Done: success, finished, restored, approved.
    Success,
    /// Merged into something else.
    Merged,
    /// In progress, or a warning.
    Warning,
    /// An error, a failure, danger.
    Danger,
    /// Information, a reference.
    Info,
    /// Waits for the chief.
    Attention,
    /// At rest or idle.
    Rest,
}

impl StatusKind {
    /// The tone of its mark, for an icon, a dot or a bar. Words beside
    /// it stay in the body tone, and a danger is red either way.
    pub fn tone(self) -> Tone {
        match self {
            Self::Success => Tone::Success,
            Self::Merged => Tone::Merged,
            Self::Warning => Tone::Warning,
            Self::Danger => Tone::Critical,
            Self::Info => Tone::Info,
            Self::Attention => Tone::Attention,
            Self::Rest => Tone::Dim,
        }
    }
}

/// Every status, each with the one meaning it has everywhere.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Statuses {
    /// Done: success, finished, restored, approved. Green.
    pub success: Status,
    /// Merged: a hire merged into another, a merge action or
    /// record. Purple.
    pub merged: Status,
    /// In progress or a warning: an agent at work, something
    /// pending, a budget near its cap. Yellow.
    pub warning: Status,
    /// An error, a failure, danger. Red.
    pub danger: Status,
    /// Information and references: a note, a tip, an id. Blue.
    pub info: Status,
    /// What needs the chief. Orange.
    pub attention: Status,
    /// At rest or idle. Neutral.
    pub rest: Status,
}

impl Statuses {
    /// How `kind` is drawn.
    pub fn get(&self, kind: StatusKind) -> &Status {
        match kind {
            StatusKind::Success => &self.success,
            StatusKind::Merged => &self.merged,
            StatusKind::Warning => &self.warning,
            StatusKind::Danger => &self.danger,
            StatusKind::Info => &self.info,
            StatusKind::Attention => &self.attention,
            StatusKind::Rest => &self.rest,
        }
    }
}

impl Palette {
    /// Every colour by name with the part it plays.
    pub fn parts(&self) -> [(&'static str, Part, Color); 33] {
        // No `..`: a new colour does not build until it has a part.
        let Self {
            surface,
            grey_1,
            button,
            button_hover,
            border,
            selected,
            gold_1,
            gold_2,
            gold_3,
            gold_4,
            gold_5,
            gold_6,
            gold_hover,
            accent,
            glow,
            glow_core,
            particle,
            primary_low,
            primary_high,
            agent,
            card,
            input,
            code,
            text_selection,
            bottom_glow,
            red,
            danger,
            danger_border,
            danger_hover,
            danger_glow,
            green,
            green_deep,
            shadow,
            // Checked by its own pairs in `contrast::Report`.
            status: _,
        } = *self;
        [
            ("surface", Part::Panel, surface),
            ("card", Part::Panel, card),
            ("input", Part::Fill, input),
            ("code", Part::Fill, code),
            ("button", Part::Fill, button),
            ("button_hover", Part::Fill, button_hover),
            ("gold_hover", Part::Fill, gold_hover),
            ("gold_6", Part::Fill, gold_6),
            ("selected", Part::Fill, selected),
            ("primary_low", Part::Fill, primary_low),
            ("danger", Part::Fill, danger),
            ("danger_hover", Part::Fill, danger_hover),
            ("green_deep", Part::Fill, green_deep),
            ("bottom_glow", Part::Fill, bottom_glow),
            ("text_selection", Part::Fill, text_selection),
            ("gold_1", Part::Ink, gold_1),
            ("grey_1", Part::Ink, grey_1),
            ("gold_2", Part::Ink, gold_2),
            ("accent", Part::Ink, accent),
            ("red", Part::Status, red),
            ("green", Part::Status, green),
            ("gold_5", Part::Mark, gold_5),
            ("gold_3", Part::Mark, gold_3),
            ("gold_4", Part::Mark, gold_4),
            ("danger_border", Part::Mark, danger_border),
            ("border", Part::Light, border),
            ("glow", Part::Light, glow),
            ("glow_core", Part::Light, glow_core),
            ("particle", Part::Light, particle),
            ("primary_high", Part::Light, primary_high),
            ("agent", Part::Light, agent),
            ("danger_glow", Part::Light, danger_glow),
            ("shadow", Part::Light, shadow),
        ]
    }
}

/// The sizes of the lettering.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSizes {
    pub body: f32,
    /// Notes, buttons, tabs, breadcrumbs and the names of a panel's
    /// parts. The least size of any lettering.
    pub small: f32,
    /// A menu's title and a card's, never below the body.
    pub heading: f32,
    /// The name at the head of a page, in the display face.
    pub title: f32,
    /// The space between the letters of the display face.
    pub tracking: f32,
    /// The weight of regular words in dark ink, above the regular
    /// 400, so dark words on a light ground read as solid as light
    /// words on a dark one.
    pub dark_ink_weight: u16,
}

/// The gaps, heights and corners of the panels.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spacing {
    pub gap: f32,
    /// The height of a row.
    pub row: f32,
    /// The corner of a control and of a card, theme round. Frames
    /// stay square.
    pub round: f32,
    /// The corner of what floats: a menu, a card over the world.
    pub soft: f32,
    pub divider: f32,
    /// How far either side of a divider the pointer takes hold of
    /// it.
    pub divider_grip: f32,
    pub tab_padding: f32,
    /// The width of a control's frame and a card's.
    pub border: f32,
    /// The space inside a panel, around its content.
    pub panel: f32,
    /// The space between the parts of a panel.
    pub section: f32,
    /// The space inside a card, across and down.
    pub card_padding_x: f32,
    pub card_padding_y: f32,
    /// The space down inside a row of a list.
    pub row_padding: f32,
    /// The height of the menu bar, and the space at its ends.
    pub bar: f32,
    pub bar_padding: f32,
    /// The space at each side of a menu's title.
    pub title_padding: f32,
    /// The least width of an open menu, and the space between an
    /// entry's words and its keys.
    pub menu_width: f32,
    pub menu_gap: f32,
    /// The space across and down inside the status bar, between its
    /// parts, the side of its icons and the space after each.
    pub status_padding_x: f32,
    pub status_padding_y: f32,
    pub status_gap: f32,
    pub status_icon: f32,
    pub status_icon_gap: f32,
    /// The widest a page of Settings is drawn, and its table of
    /// models, whose columns need more.
    pub page_width: f32,
    pub table_width: f32,
    /// The widest a card over the panels is drawn, and the space
    /// inside it.
    pub overlay_width: f32,
    pub overlay_padding: f32,
    /// The least height of a tab within a panel, and the line under
    /// the one shown.
    pub tab_height: f32,
    pub underline: f32,
    /// The share of a panel's height its background keeps before it
    /// shifts toward the bottom glow.
    pub bottom_start: f32,
    /// The width and height of a switch's track, and the space
    /// between its frame and its knob.
    pub switch_width: f32,
    pub switch_height: f32,
    pub switch_inset: f32,
    /// The space between the bar's edge and the desks' track,
    /// between the track's frame and its segments, and the track's
    /// corner. A segment's corner follows the track's.
    pub track_inset: f32,
    pub track_padding: f32,
    pub track_radius: f32,
    /// The height of the line that sets the track apart from the
    /// menus.
    pub track_divider: f32,
}

/// How far the shadow under what floats falls, and how softly. Its
/// colour is the palette's.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fall {
    /// How far below what floats it falls.
    pub drop: f32,
    pub blur: f32,
}

/// How long the panels take to move.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    /// A control answering the hand.
    pub interact: f32,
    /// Something opening or folding away.
    pub expand: f32,
}

/// Another theme is shown than the one the panels were built with.
/// Fynix lends [`bevy_fynix::Theme`] out each update, which marks it
/// changed, so this is how a change is told.
#[derive(Message, Clone, Copy, Debug)]
pub struct ThemeChanged;

/// Shows `theme`, for the views and the window behind them, and the
/// first time lets the panels be built. A theme equal to the shown
/// one changes nothing.
pub fn show(world: &mut World, theme: HamletTheme) {
    let shown =
        world.get_resource::<bevy_fynix::Theme<HamletTheme>>();
    if shown.is_some_and(|shown| shown.0 == theme) {
        return;
    }
    world.insert_resource(ClearColor(theme.background()));
    world.insert_resource(bevy_fynix::Theme(theme));
    // A bare test app has neither.
    if world.contains_resource::<Messages<ThemeChanged>>() {
        world.write_message(ThemeChanged);
    }
    if world
        .get_resource::<State<ThemeState>>()
        .is_some_and(|state| *state.get() == ThemeState::Loading)
    {
        world
            .resource_mut::<NextState<ThemeState>>()
            .set(ThemeState::Ready);
    }
}

impl HamletTheme {
    pub fn new(palette: Palette, measures: &Measures) -> Self {
        let Measures {
            text,
            space,
            shadow,
            motion,
        } = measures.clone();
        Self {
            palette,
            text,
            space,
            shadow,
            motion,
        }
    }

    /// Hamlet's dark variant in the designed measures.
    pub fn designed() -> Self {
        let hamlet = themes::hamlet();
        let palette =
            hamlet.variants.get(WindowTheme::Dark).palette.clone();
        Self::new(palette, &Measures::designed())
    }

    /// The window background, behind the panels.
    pub fn background(&self) -> Color {
        self.palette.surface
    }

    /// The shadow of what floats.
    pub fn shadow(&self) -> BoxShadow {
        let shadow = &self.shadow;
        BoxShadow::new(
            self.palette.shadow,
            px(0.0),
            px(shadow.drop),
            px(0.0),
            px(shadow.blur),
        )
    }
}

impl TextTokens for HamletTheme {
    fn tone(&self, tone: Tone) -> Color {
        let palette = &self.palette;
        match tone {
            Tone::Body => palette.gold_1,
            Tone::Dim => palette.grey_1,
            // Gold 2 marks what can be pressed.
            Tone::Faint => palette.gold_2,
            Tone::Accent => palette.accent,
            Tone::OnAccent => contrast::clearer(
                palette.surface,
                palette.gold_1,
                palette.gold_4,
            ),
            Tone::Critical => palette.status.danger.ink,
            Tone::Success => palette.status.success.mark,
            Tone::Merged => palette.status.merged.mark,
            Tone::Warning => palette.status.warning.mark,
            Tone::Info => palette.status.info.mark,
            Tone::Attention => palette.status.attention.mark,
            Tone::Reference => palette.status.info.ink,
            Tone::OnSolid => palette.status.success.on_solid,
        }
    }

    fn body_size(&self) -> f32 {
        self.text.body
    }

    fn small_size(&self) -> f32 {
        self.text.small
    }
}

impl SurfaceTokens for HamletTheme {
    fn fill(&self) -> Color {
        self.palette.button_hover
    }

    fn hover(&self) -> Color {
        self.palette.button
    }

    fn pressed(&self) -> Color {
        self.palette.button_hover.mix(&self.palette.selected, 0.5)
    }

    fn current(&self) -> Color {
        self.palette.selected
    }

    fn panel(&self) -> Color {
        self.palette.surface
    }

    fn hairline(&self) -> Color {
        self.palette.border
    }

    fn selection(&self) -> Color {
        self.palette.gold_6
    }

    #[expect(
        clippy::misnamed_getters,
        reason = "fynix's accent is a fill; the palette's is an ink"
    )]
    fn accent(&self) -> Color {
        self.palette.gold_4
    }

    /// The accent fill under the pointer, moved away from the
    /// on-accent tone so its words keep their contrast.
    fn accent_hover(&self) -> Color {
        self.away_from_on_accent(0.08)
    }

    fn accent_pressed(&self) -> Color {
        self.away_from_on_accent(0.1)
    }
}

/// How far above [`contrast::TEXT`] the words of an empty field are
/// held.
const HINT_MARGIN: f32 = 2.0;

impl HamletTheme {
    /// How opaque the words of an empty field are: as faint as dim
    /// text can be while it still reads on the field.
    pub fn hint_opacity(&self) -> f32 {
        (0..=100)
            .map(|step| step as f32 / 100.0)
            // Above the need by enough that thin, antialiased letters
            // still meet it on screen.
            .find(|&opacity| {
                contrast::contrast(
                    self.hint(opacity),
                    self.palette.input,
                ) >= contrast::TEXT + HINT_MARGIN
            })
            .unwrap_or(1.0)
    }

    /// Dim text at `opacity` as seen on a field.
    pub(crate) fn hint(&self, opacity: f32) -> Color {
        let ground = Srgba::from(self.palette.input);
        ground
            .mix(&Srgba::from(self.palette.grey_1), opacity)
            .into()
    }

    fn away_from_on_accent(&self, by: f32) -> Color {
        let accent = self.accent();
        if contrast::light(self.tone(Tone::OnAccent))
            < contrast::light(accent)
        {
            accent.lighter(by)
        } else {
            accent.darker(by)
        }
    }
}

impl SpacingTokens for HamletTheme {
    fn gap(&self) -> f32 {
        self.space.gap
    }

    fn row(&self) -> f32 {
        self.space.row
    }

    fn radius(&self) -> f32 {
        self.space.round
    }

    fn divider(&self) -> f32 {
        self.space.divider
    }

    fn divider_grip(&self) -> f32 {
        self.space.divider_grip
    }

    fn tab_padding(&self) -> f32 {
        self.space.tab_padding
    }

    fn menu_radius(&self) -> f32 {
        self.space.soft
    }
}

impl MotionTokens for HamletTheme {
    fn motion(&self, motion: Motion) -> Curve {
        let seconds = match motion {
            Motion::Interact => self.motion.interact,
            Motion::Expand => self.motion.expand,
        };
        Curve {
            duration: Duration::from_secs_f32(seconds),
            ease: |t| 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}
