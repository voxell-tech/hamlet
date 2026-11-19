---
status: accepted
---

# The tree of agents stays a tree under one lead

An agent's place is one column, `hamlet_agent.parent`: the lead has none,
every other agent has exactly one living parent of its own hamlet. Tree
channels are derived from it, never stored, and the allowance chain (H44)
is read from it at spend time, so moving an agent is one update and nothing
else has to be rewritten.

Reparenting (H41) keeps that shape. It refuses to move the lead (the office
moves by H18), to put an agent under itself or anything beneath it (checked
by walking up from the new parent), or to cross hamlets. It is allowed to a
chief, the Hand, or an agent above both the agent and the new parent, so
no parent can take in agents from outside its subtree or hand its own to a
stranger.

Opened and agreed channels belong to the pair, not to the tree, and a move
leaves them, and what they cost, as they were. Spend stays charged where it
was spent; only the parents whose allowances bind the agent change.

Moving is hard to undo cleanly once agents have worked under their new
parent: the record keeps the old parent, but nothing moves a channel or a
charge back.
