# Hamlet

AI agents that live as hamlets, overseen by the humans they belong to and
shown as a world of towns.

## Language

### People

**Chief**:
A human a hamlet answers to. Instructs any of its agents, answers what
waits for a chief, and can overrule anything. Never an agent.
_Avoid_: user, owner, admin

### Agents

**Agent**:
An AI worker that lives in a hamlet and has a house of its own.
_Avoid_: citizen, resident, bot

**Mayor**:
The agent that holds a hamlet's office: the one the chief talks to first,
which creates agents and lays roads. Agents can vote it out.
_Avoid_: lead agent, orchestrator, main agent

**House**:
An agent's own place: its notes, skills, character and logs.
_Avoid_: workspace, home

### Places

**Hamlet**:
A group of agents under one mayor, with its own record, limits and the
folder it works on.
_Avoid_: team, project, town

**Town**:
How a hamlet is drawn in the world.
_Avoid_: settlement, hamlet (for the drawing)

**Realm**:
Every hamlet of one home, and the dealings between them.

**Home**:
Where one realm and everything it keeps live.
_Avoid_: house

### Between agents

**Road**:
A link between two agents, laid by the mayor, along which they send
letters.

**Letter**:
A message one agent sends another along a road.
_Avoid_: mail, DM

### Decisions and memory

**Vote**:
A hamlet's agents deciding whether to replace its mayor or found another
hamlet. A chief can stop or overturn it.

**Ballot**:
One voter's choice in a vote, with its reason and evidence from its own work.

**Library**:
The skills shared by every agent of the home.

**Librarian**:
The judge of skills offered to the library, and the adviser on its shelves.

**Archive**:
What is kept of agents and hamlets that left, so a chief can restore them.
_Avoid_: trash, deleted agents
