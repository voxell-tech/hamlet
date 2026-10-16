# The SDK

`hamlet_sdk` is what a feature of the window is built with. A feature is a
crate that lists `hamlet_sdk` and the crates below it, and never
`hamlet_ui`, so Cargo refuses a feature that reaches into the window's
private code. `hamlet_petition` is the first.

## What it holds

| Module | What a feature takes from it |
|---|---|
| `theme` | `HamletTheme`, the palette and tones, read from `assets/theme.toml` |
| `looks` | The looks of a button, a field, a title and a list, and the helpers that set an icon beside words |
| `icons` | `Glyph`, every icon the panels draw |
| `feel` | `Lit` and `Mood`: how a control or a panel answers the hand, tuned by `assets/feel.toml` |
| `platform` | The typefaces and the small rules of a node that fynix leaves to the app |
| `tabs` | A row of tabs within a panel |
| `dock` | `add_panel`, `panel`, `lit_panel`, `show_tab` and the `TabShown` event |
| `cue` | Writing a sound cue |
| `convo` | `Convo`, the agent the chat is with |

The root has `View`, `Cx`, `AnyView` and `with_cx`, and `plugin`, which adds
what every panel is built on. The app adds `theme::plugin`, which loads
the theme.

## Adding a panel

A panel is a crate with a `plugin` that registers its dock window:

```rust
pub fn plugin(app: &mut App) {
    app.add_panel(
        "needs",
        DockWindowKind::new("PETITIONS", || lit_panel(screen(), Lit::default()))
            .icon(Glyph::Bell.image(ICON)),
    );
}
```

The window adds the plugin to its app, and `dock::seed` in `hamlet_ui` says
where the panel starts. A panel with no place there is closed until it is
asked for from the View menu or a tab bar's plus.

## Where plugins meet

A feature does not call another feature. It reads the SDK's state, or it
offers a seam:

- `TabShown` is triggered when a panel's tab is brought forward. The window
  reads the lists again as `"agent"` comes forward.
- `hamlet_petition::AboutAppExt::add_about` lets a plugin draw what a
  petition is about. The chat adds the change of an edit that waits.

A seam is added when a second plugin needs it, not before.

## A feature's own tuning

A crate that has values to tune owns its file under `assets/`, as the feel
owns `feel.toml`, and its settings struct reads it with `Tuning` and
`designed!`. A section of another crate's file is not shared.
