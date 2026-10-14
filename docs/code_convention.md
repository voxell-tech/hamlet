# Code convention

## Confirm before reaching for memory-management types

`Arc`, `Rc`, `Box<dyn Trait>`, `Mutex`, `RwLock`, `Cell`, `RefCell`, or
any other memory-management/interior-mutability type: stop and confirm
with the user before introducing one. They carry a real runtime cost -
an allocation, a reference count, a lock - and are easy to reach for
out of habit where plain ownership or borrowing would have worked;
overused, they add up and slow the program down. Explain what plain
ownership or borrowing was tried first and why it didn't work, and let
the user decide rather than reaching for one of these as the default
fix.

## Turbofish, not an annotated binding

When a generic call's type needs pinning down, prefer turbofish on
the call itself (`.collect::<Vec<_>>()`, `.parse::<i32>()`, and so on)
over annotating the binding's type to steer inference.

## Colours are parameters of a shader, not constants in it

A shader takes its colours as uniforms, set from Rust. This holds
above all for colours that are a matter of taste or of the theme: the
palette, the accents, anything a setting might one day change. A
`const` colour in WGSL can't be animated, themed or changed at run
time, and it hides from anyone reading the theme or the palette.

The values live where the other colours live (`ui/theme.rs` for the
panels, `assets/palette.toml` for the world) and reach the shader through
the material. Numbers that are part of the shader's own arithmetic,
such as a falloff or a threshold, may stay in the shader.

## Plugins are added, not called

A module's plugin is a `fn plugin(app: &mut App)` or a `Plugin` struct, and
either way it is added with `app.add_plugins((.., flag::plugin, ..))`. Do
not call it as a function (`flag::plugin(app)`): added, it is named in the
app's list of plugins and cannot be added twice by mistake.

## The plugin comes first

A module's plugin comes before its resources, components, systems and
other structs, enums and functions, so what the module adds to the app is
read before how it works. Only constants, statics and type aliases may
stand above it, under the `use` and `mod` lines. A `Plugin` struct is
followed by its impls, with nothing else between them.

## A tuning value is written once, in its file under `assets/`

A size, a speed, a timing or a colour of the world goes in the file for
its subject under `assets/`, and its field goes in that subject's settings
struct with a doc comment saying what it is. The struct's `Default` reads
the file (`hamlet_assets::Tuning`), so do not write the value again in
code, in a `Default` or anywhere else.

Shaders are the exception to "assets are files under `assets/`": a shader
stays beside the code that binds it and is embedded from there.

## No mod.rs

Use the filename instead of `mod.rs` in a folder.
