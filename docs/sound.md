# Sound

Everything heard in the app is made in code, in
`hamlet_realm/src/sound/`: no samples, no files. Settings has two
switches, Sound for the effects and the ambience and Music for the music.

Everything with a note is in **D major**, mostly on its pentatonic scale
(D E F# A B), so an effect and the music can sound together without
clashing.

## How a sound gets played

Whoever sees a thing happen writes a `Cue { sound, at, turn }`. The `play`
system in `mix.rs` plays it where it happened, heard from ears placed
between the camera and the ground it looks at. Sounds of the panels and of
the chief's answers are heard the same wherever the camera is. The panels
know nothing of the realm's sounds: they write a `PanelCue` of a
`PanelSound` (`hamlet_realm_api::sound`), which `heard::hear_panels` turns
into the `Cue` of the same name at the listener.

Each playing is varied by its `turn`. A sound with no note strays a little
in pitch, and a sound with a note moves only to other notes of the scale.

A cue is left out when its family is full, when the same sound has just
played, when it is out of earshot, or when it is a second alert within a
few seconds.

## The sounds

`kinds.rs` names every sound and its family. `synth.rs` has the parts a
clip is built from (struck wood, plucked tines, bars, bells, glass, noise)
and `synth/` builds the clips.

| Family | What is in it |
|---|---|
| `ambience` | Birds by day, crickets at night, wind always |
| `steps`, `work`, `doors` | A robot's feet, its chores, its door, a letter changing hands |
| `alerts` | An agent begins to wait for the chief, or faults |
| `answers` | The chief allows, denies or stops |
| `hand` | What the chief's hand does in the world |
| `magic` | Houses rising and falling, merges, roads, dawn and dusk |
| `talk` | Speech and thought balloons |
| `lives` | Agents born, restored and gone, votes, a new mayor, a founding |
| `interface` | The panels: buttons, menus, tabs, sending a message |

The alert and the answers are written as a pair: the chime leaves a
question open on E and B, and "allowed" closes it on the home chord.

To add a sound, name it in `kinds.rs`, build its clip in `synth/` and
write a `Cue` where it happens. World events are in
`generator/sounds.rs`. A panel sound is named in `PanelSound` too and
mapped in `hear_panels`; the window's own are in `hamlet_ui/src/sounds.rs`,
the menus' in `plugins/menus/src/lib.rs`.

The chief's choice of what is heard, from Settings and the saved view, is
`SoundChoice`; the realm carries it into `SoundSettings`.

## The music

`music.rs` writes it and `voices.rs` plays it: a kalimba, a music box, a
small bell, a soft bass and a pad.

It is made a **breath** at a time. A breath is one chord held as a pad
that swells and goes, with a few plucked notes over it. Each is rendered
on a background task a few seconds before it is due. Every breath begins
and ends in silence and overlaps the one before, so there is no loop and
no edge where a click could be.

- **Harmony** walks among nine chords, weighted by the light, and comes
  home to D at least every fifth breath.
- **Time of day**: by day higher and brighter. At night the chords are
  minor, the tune is lower and rarer, and there is real silence between
  breaths for the crickets.
- **Activity**: agents at work add notes and shorten the rests. All at
  rest, it thins to a note every several seconds.
- **Moments**: a house settled, the chief's yes or no, a founding, dawn
  and dusk are each answered by a short phrase in the chord that is
  playing.

## Settings

All in `SoundSettings` in `sound.rs`: a volume for each family and for the
music, how many of a family play at once, and how soon one repeats. Each
is explained beside its field.

## Render the sheet

```sh
cargo run --release --example sound_sheet            # into target/sound-sheet
```

It opens no audio device and no window. It writes a WAV of every effect,
the music in six moods, and a scene of music with effects over it. It
prints the level of each and warns of a click, a hard edge, or an effect
much louder than its family.
