# Proposal: what the actor runtime does not do yet

The actor runtime in [`docs/spec/actor-runtime.md`](../spec/actor-runtime.md)
is built. These parts of its design are not.

- **Durable sends.** A send would be a row of an outbox, committed with the
  change that caused it and sent again until its receiver records that it
  handled it, with a sequence number per pair of actors so sends arrive in
  order. As built, every message between actors is a call with a deadline, and
  the realm's ordered deliveries (A13) are held in memory while it runs.
- **A turn's sends commit with its tool results.** pi-durable would commit
  the outbox rows a tool call makes in the same commit as the call's result.
  As built, a tool call cut off by a stop runs again under its id, and the
  realm and the workspace answer it from their markers (A9, A16).
- **Owned work across hosts.** A row for each model turn and command would
  name its owner and the host's boot id, and a host that finds a row from an
  older boot would tell the owner once that the work was cut off. As built, the
  workspace marks its own commands cut off at its start (A15), and the link
  carries no boot id.
- **A host index and an address directory.** A table per host would hold
  each actor's next deadline and whether it has sends or open work, so a
  boot reads one table rather than every file. A send to a host that is not
  linked would wait in the outbox.
- **Dead letters.** A send that keeps killing its host would go, after a
  fixed number of tries, to a dead-letter table, and the chief would be told.
  As built, only a one-shot timer is set aside after its tries (A14).
- **Forks of an agent.** A fork into a new agent would copy the source
  agent's file with `VACUUM INTO`, and the new agent's first start would drop
  the copied sends and timers and mark a copied turn cut off.
- **A remote workspace that runs tools.** A chief can choose a remote
  workspace and copy every agent's filesystem to it, but every agent's tools
  run in the local workspace actor.

## A known gap: an alarm set directly

An alarm an actor sets with `ActorContext::set_alarm` alone, with no
schedule row behind it, lives only in the running core. The host records
the last one when the actor stops cleanly. A kill before that stop loses
it: the next start arms the actor's alarms again from its schedule rows, and
this alarm has none. An alarm set through a schedule (`ctx.after`) has its
row in the actor's own file and is not lost. A raw `set_alarm` needs a row
of its own, or to be kept from actors' code.
