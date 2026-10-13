# What an agent's tools can reach

## The sandbox

Tools run in a sandbox: bubblewrap on Linux, Seatbelt on macOS. Windows has
no sandbox yet and needs `--insecure`. An agent's tools can write only in
its own workspace, and a command that reaches past the sandbox waits in
Petitions for the chief.

Tools have no network access until "Tools network access" is turned on
under Agents in Settings, or `HAMLET_NET=1` is set.

`crates/hamlet_sandbox/` holds the sandboxes and the policy a command runs
under.

## A folder a hamlet works on

A hamlet can work on a folder of the chief's, as a coding agent does. On
the hamlet's page, "Choose a folder" points it at one, and every agent of
that hamlet then runs its commands there and may edit its files. Nothing of
Hamlet's is put in the folder: each agent's notes, skills and logs stay in
its own house.

A hamlet starts in manual mode. Each edit and each command waits in
Petitions for the chief to allow it once, for the rest of the agent's
session, or not at all. Reading is free.

In auto mode the agents edit and run without asking, and the sandbox keeps
their writes inside the folder and their own workspace. There, a command
can read the rest of the machine, so the project's own tools run, but it
cannot read Hamlet's home, and it can never write `.git/hooks` or
`.git/config`.

Both are seen on the workbench in the middle of the hamlet's common. The
bench stands for the folder: it is bare with none, and has work laid out
on it and the folder's name over it when there is one. A click on it
shows the hamlet's page. The clockwork on the bench shows the mode: its
wheels turn by themselves under a blue lamp in auto, and stand still
under an orange lamp in manual, with the crank waiting. With no folder
the lamp is dark. A click on the clockwork switches the mode.

[`tour.md`](tour.md) names the code for each of these under "The folder a
hamlet works on".
