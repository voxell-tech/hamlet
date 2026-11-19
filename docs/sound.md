# Sound

Everything heard in the app is made in code, in `hamlet_realm/src/sound/`: no
samples, no files. Settings has two switches, Sound for the effects and Music
for the music. Ambience has a switch of its own, and Sound off silences it.

Everything with a note is in D major, mostly on its pentatonic scale (D E F#
A B), so an effect and the music can sound together without clashing.

## How a sound gets played

Whoever sees a thing happen writes a `Cue { sound, at, turn }`. The `play`
system in `mix.rs` plays it where it happened, heard from ears placed between
the camera and the ground it looks at. Sounds of the panels and of the chief's
answers are heard the same wherever the camera is.

Each playing is varied by its `turn`: a sound with a note moves only to other
notes of the scale. A cue can be left out when its family is off or full or
the same sound has just played. Volumes and limits are in `SoundSettings` in
`sound.rs`.

## Adding a sound

`kinds.rs` names every sound and its family; `synth.rs` has the parts a clip
is built from and `synth/` builds the clips. To add a sound, name it in
`kinds.rs`, build its clip in `synth/` and write a `Cue` where it happens.
World events are in `generator/sounds.rs` and panel sounds in
`hamlet_app/src/sounds.rs`.

## The music

`music.rs` writes it and `voices.rs` plays it, a **breath** at a time, each
rendered on a background task a few seconds before it is due. Every breath
begins and ends in silence and overlaps the one before, so there is no loop
and no edge where a click could be. Harmony comes home to D at least every
fifth breath.

## Render the sheet

```sh
cargo run --release --example sound_sheet            # into target/sound-sheet
```

It opens no audio device and no window. It writes a WAV of every effect and
the music, and warns of a click, a hard edge, or an effect much louder than
its family.
