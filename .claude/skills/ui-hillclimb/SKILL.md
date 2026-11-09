---
name: ui-hillclimb
description: Improve how Hamlet's UI looks in rounds, with one agent that renders and scores the screens and another that fixes what it finds, until a target score. Use when the owner asks to hill-climb, polish or review the look of the app, a theme, light or dark mode, or a set of screens.
---

# UI hill-climb

Two agents take turns: a reviewer renders the screens, measures and
scores them, and a fixer changes the code. The lead, the session that
started the climb, only passes work between them and talks to the
owner. Each runs in a thread of its own, so the owner can read each one
apart.

## The two agents

- **The reviewer** renders, measures and scores. It never edits app
  code, themes or the gallery. Start a new reviewer each round. It sees
  the rubric, the owner's reference screens and the new renders, but not
  what was changed or why, so each score is a fresh look.
- **The fixer** changes code and themes. One fixer stays for the whole
  climb, so it keeps what it learned. It gets each round's score, the
  tables of scores by criterion and by part, and the findings, but not
  the rubric. It works on the problems and does not tune for the scoring
  method.

## The rubric

The lead writes it once for the goal, in a scratch folder outside the
repository, before round 0. It gives:

- the criteria, each with a weight and what a 10 looks like;
- what a 9 needs, such as every heavy criterion at 8 or above;
- the hard rules, each with the score it caps;
- the principles of any reference screens the owner gave. Describe the
  principles, never the products.

Example hard rule: light and dark are one design. They have the same
elements, shapes and layering, and only colour values differ. An element
or treatment that exists in one mode only caps the score at 6.

## The reviewer's report

`findings.md` for each round holds:

1. `**Score: N / 10**`;
2. a table by criterion: weight, score and a one-line reason;
3. a table by screen part: score and the worst measured ratio, with the
   other mode's in brackets;
4. a table by scene, when every scene is rendered;
5. findings, worst first. Each gives the shot, pixel coordinates, the
   measurement, and what is wrong and what it should look like, in words
   a fixer can act on without the rubric. It never prescribes code.

## Tools

- `shoot.sh <out> <light|dark> <scene>...` renders `ui_gallery` scenes
  offscreen, with no window, so nothing on the owner's screen is
  captured. The gallery's scenes are listed in
  `crates/hamlet_app/examples/ui_gallery/`. A state the rubric needs but
  no scene shows is added to the gallery, not to the app.
- `sample.swift` reads pixels from a PNG and prints their colours and the
  WCAG contrast between pairs, so ratios are measured, not guessed.
- The theme's contrast gate is `crates/hamlet_ui/src/theme/contrast.rs`.

## The loop

1. Round 0: the reviewer renders every scene in every mode the goal
   covers, scores, and writes `round0/findings.md`.
2. The lead sends the fixer the score, both tables and the findings path.
3. The fixer fixes the worst findings first, runs `cargo +nightly fmt`
   and `cargo clippy --all-targets` for each crate it touched, and
   commits. It renders nothing and runs no tests.
4. A new reviewer scores the next round, into `round<N>/`.
5. Repeat until the target score. Stop early after two rounds with no
   gain, or after eight rounds, and take the scores and the open
   questions to the owner.

After every review the lead posts to the owner: the score, the parts
that moved, the key screenshots of the round inline (light beside dark
for the main scenes), and any call that is the owner's to make, such as
removing something the owner asked for. Changes to the brief go to the
running agent as a message, and into the rubric for later rounds.

## Rules for the fixer

- Fix causes, not symptoms: one shared rule over patches per widget.
- Edit code with the Edit tool. Check with fmt and clippy only.
- `vendor/fynix` may change when it is the right place. Commit it on a
  branch inside the submodule, then commit the new pointer in Hamlet.
- Commit only its own paths, with one line under ten words. Never push
  or open a pull request.
- Never take something away that the owner asked for. Bring it to the
  lead instead.
