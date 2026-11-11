-- The realm's database. Times are milliseconds since the epoch. An
-- actor column holds `chief:<id>`, `agent:<id>` or `realm`. Rows of
-- scholars, agents, conversations, tasks and waits are never deleted,
-- so their ids are never given twice.

-- The realm

-- The realm's identity, shared with its realm.id file, and the
-- version of the schema it was made with, in semver.
CREATE TABLE realm (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    identity TEXT NOT NULL,
    version TEXT NOT NULL,
    made INTEGER NOT NULL
) STRICT;

CREATE TABLE chief (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    since INTEGER NOT NULL
) STRICT;

-- The chief's documents: text for the charter and the librarian, JSON
-- for the settings, the catalogue and the realm's limits.
CREATE TABLE document (
    kind TEXT PRIMARY KEY CHECK (kind IN
        ('charter', 'settings', 'catalogue', 'librarian', 'limits')),
    value TEXT NOT NULL,
    at INTEGER NOT NULL,
    chief INTEGER REFERENCES chief(id)
) STRICT, WITHOUT ROWID;

-- The city

-- A scholar, or the Hand's own knowledge. Its repository is
-- scholars/<id>.git. Its name is a label that may repeat.
CREATE TABLE scholar (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('scholar', 'hand')),
    name TEXT NOT NULL,
    chief INTEGER REFERENCES chief(id),
    since INTEGER NOT NULL,
    made_by TEXT NOT NULL,
    planted INTEGER NOT NULL DEFAULT 0 CHECK (planted IN (0, 1)),
    -- Whether it lives in the city or waits at its harbor, or is a
    -- blank hire's own and never of the city.
    arrival TEXT NOT NULL DEFAULT 'housed'
        CHECK (arrival IN ('housed', 'waiting', 'turned', 'apart')),
    -- Who last let it in, kept it waiting or turned it away; none
    -- while it is as it was made.
    arrival_by TEXT,
    -- Its house's spot from the city's middle and the way its door
    -- faces, while it lives in the city.
    house_x REAL,
    house_z REAL,
    house_turn REAL,
    fork_of INTEGER REFERENCES scholar(id),
    fork_at TEXT,
    left_at INTEGER,
    left_by TEXT,
    left_reason TEXT,
    erased INTEGER NOT NULL DEFAULT 0 CHECK (erased IN (0, 1)),
    CHECK ((kind = 'hand') = (chief IS NULL)),
    CHECK (erased = 0 OR left_at IS NOT NULL)
) STRICT;
CREATE INDEX scholar_chief ON scholar(chief);

-- Hamlets

-- A hamlet: a UUID made when it is founded, and the name it is shown
-- by. Its mayor is the root of its tree of hires. Its limits are the
-- chief's document for it.
CREATE TABLE hamlet (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    founded_at INTEGER NOT NULL,
    founded_by TEXT NOT NULL,
    mayor INTEGER NOT NULL
        REFERENCES hire(agent) DEFERRABLE INITIALLY DEFERRED,
    folder TEXT,
    mode TEXT NOT NULL DEFAULT 'manual' CHECK (mode IN ('manual', 'auto')),
    tier INTEGER NOT NULL DEFAULT 50 CHECK (tier IN (50, 100, 200, 500)),
    site_x REAL,
    site_z REAL,
    limits TEXT NOT NULL CHECK (json_valid(limits)),
    removed_at INTEGER,
    removed_by TEXT,
    removed_reason TEXT,
    erased INTEGER NOT NULL DEFAULT 0 CHECK (erased IN (0, 1)),
    CHECK (erased = 0 OR removed_at IS NOT NULL)
) STRICT, WITHOUT ROWID;

-- The chiefs a hamlet answers to.
CREATE TABLE hamlet_chief (
    hamlet TEXT NOT NULL REFERENCES hamlet(id),
    chief INTEGER NOT NULL REFERENCES chief(id),
    PRIMARY KEY (hamlet, chief)
) STRICT, WITHOUT ROWID;

-- The folders a hamlet worked on, and when it last chose each.
CREATE TABLE hamlet_folder (
    hamlet TEXT NOT NULL REFERENCES hamlet(id),
    folder TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (hamlet, folder)
) STRICT, WITHOUT ROWID;

-- Agents

-- Anything that talks: the Hand, a hire or a worker. `harness` is the
-- one its new conversations run in. `effort` is how hard its model
-- thinks when chosen for it, and `fast` asks the provider for its fast
-- tier.
CREATE TABLE agent (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('hand', 'hire', 'worker')),
    since INTEGER NOT NULL,
    harness TEXT NOT NULL,
    effort TEXT CHECK (effort IS NULL OR effort IN
        ('off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max')),
    fast INTEGER NOT NULL DEFAULT 0 CHECK (fast IN (0, 1)),
    ended_at INTEGER,
    ended_by TEXT,
    ended_how TEXT CHECK (ended_how IS NULL OR ended_how IN
        ('ended', 'merged', 'scholar', 'hamlet', 'answered', 'failed',
         'stopped')),
    ended_reason TEXT,
    erased INTEGER NOT NULL DEFAULT 0 CHECK (erased IN (0, 1)),
    CHECK ((ended_at IS NULL) = (ended_how IS NULL)),
    CHECK (erased = 0 OR ended_at IS NOT NULL)
) STRICT;
CREATE UNIQUE INDEX agent_hand ON agent(kind) WHERE kind = 'hand';

CREATE TABLE hand (
    agent INTEGER PRIMARY KEY REFERENCES agent(id),
    scholar INTEGER NOT NULL UNIQUE REFERENCES scholar(id),
    -- The model chosen for it; the realm's default while none.
    model TEXT
) STRICT;

-- A scholar working in a hamlet. Its branch is hires/<agent> in its
-- scholar's repository and its lodge hamlets/<hamlet>/<agent>/. A
-- hire with no parent is under the Hand. `base` is the commit of
-- `main`, or of its pin, its branch last took in.
CREATE TABLE hire (
    agent INTEGER PRIMARY KEY REFERENCES agent(id),
    hamlet TEXT NOT NULL REFERENCES hamlet(id),
    scholar INTEGER NOT NULL REFERENCES scholar(id),
    parent INTEGER REFERENCES hire(agent),
    purpose TEXT NOT NULL,
    -- What the chief or an agent above it calls it in the world.
    nickname TEXT CHECK (nickname IS NULL OR length(nickname) > 0),
    pin TEXT,
    base TEXT,
    model TEXT,
    model_by TEXT,
    model_reason TEXT,
    allowance INTEGER CHECK (allowance IS NULL OR allowance >= 0),
    allowance_by TEXT,
    merged_into INTEGER REFERENCES hire(agent),
    CHECK (parent IS NULL OR parent <> agent)
) STRICT;
CREATE INDEX hire_scholar ON hire(scholar);
CREATE INDEX hire_parent ON hire(parent);

-- A built-in or society tool a lead turned off for a hire.
CREATE TABLE hire_tool_off (
    hire INTEGER NOT NULL REFERENCES hire(agent),
    tool TEXT NOT NULL,
    at INTEGER NOT NULL,
    by TEXT NOT NULL,
    PRIMARY KEY (hire, tool)
) STRICT, WITHOUT ROWID;

-- A worker, started by an agent or by a chief for one task described
-- by `brief`. `tools` are the built-in tools it was given.
CREATE TABLE worker (
    agent INTEGER PRIMARY KEY REFERENCES agent(id),
    starter INTEGER REFERENCES agent(id),
    chief INTEGER REFERENCES chief(id),
    hamlet TEXT REFERENCES hamlet(id),
    brief TEXT NOT NULL,
    tools TEXT NOT NULL CHECK (json_valid(tools)),
    model TEXT,
    waits INTEGER NOT NULL CHECK (waits IN (0, 1)),
    call_runtime TEXT,
    call_key TEXT,
    answer TEXT,
    -- What the chief calls it in the world.
    nickname TEXT CHECK (nickname IS NULL OR length(nickname) > 0),
    CHECK ((starter IS NULL) <> (chief IS NULL))
) STRICT;
CREATE INDEX worker_starter ON worker(starter);

-- Roads

-- A road laid between two hires of a hamlet, or agreed between hires
-- of two hamlets. Roads along the tree are not kept.
CREATE TABLE road (
    id INTEGER PRIMARY KEY,
    a INTEGER NOT NULL REFERENCES hire(agent),
    b INTEGER NOT NULL REFERENCES hire(agent),
    kind TEXT NOT NULL CHECK (kind IN ('laid', 'agreed')),
    laid_at INTEGER NOT NULL,
    laid_by TEXT NOT NULL,
    reason TEXT NOT NULL,
    task INTEGER REFERENCES task(id),
    inherited_from INTEGER REFERENCES road(id),
    up_at INTEGER,
    up_by TEXT,
    up_reason TEXT,
    CHECK (a < b)
) STRICT;
CREATE UNIQUE INDEX road_open ON road(a, b) WHERE up_at IS NULL;
CREATE INDEX road_b ON road(b);

-- One mayor's agreement to a road between hamlets. The road is laid
-- when both mayors agree.
CREATE TABLE agreement (
    id INTEGER PRIMARY KEY,
    a INTEGER NOT NULL REFERENCES hire(agent),
    b INTEGER NOT NULL REFERENCES hire(agent),
    hamlet TEXT NOT NULL REFERENCES hamlet(id),
    mayor INTEGER NOT NULL REFERENCES hire(agent),
    at INTEGER NOT NULL,
    reason TEXT NOT NULL,
    task INTEGER REFERENCES task(id),
    road INTEGER REFERENCES road(id),
    CHECK (a < b)
) STRICT;
CREATE UNIQUE INDEX agreement_open ON agreement(a, b, hamlet)
    WHERE road IS NULL;

CREATE TABLE road_charge (
    id INTEGER PRIMARY KEY,
    road INTEGER NOT NULL REFERENCES road(id),
    hamlet TEXT NOT NULL REFERENCES hamlet(id),
    agent INTEGER REFERENCES agent(id),
    task INTEGER REFERENCES task(id),
    at INTEGER NOT NULL,
    micros INTEGER NOT NULL CHECK (micros >= 0)
) STRICT;
CREATE INDEX road_charge_hamlet ON road_charge(hamlet, at);

-- The record

-- A decision of a chief, the Hand or a lead, kept with its reason
-- (H17).
CREATE TABLE decision (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    hamlet TEXT REFERENCES hamlet(id),
    actor TEXT NOT NULL,
    kind TEXT NOT NULL,
    subject INTEGER REFERENCES agent(id),
    other INTEGER REFERENCES agent(id),
    scholar INTEGER REFERENCES scholar(id),
    reason TEXT NOT NULL,
    task INTEGER REFERENCES task(id),
    detail TEXT CHECK (detail IS NULL OR json_valid(detail))
) STRICT;
CREATE INDEX decision_hamlet ON decision(hamlet, at);
CREATE INDEX decision_subject ON decision(subject);
CREATE INDEX decision_scholar ON decision(scholar);

-- Two hires merged: the commit that brought the absorbed hire's files
-- into the kept hire's branch, and the one that took them out again.
CREATE TABLE hire_merge (
    id INTEGER PRIMARY KEY,
    kept INTEGER NOT NULL REFERENCES hire(agent),
    absorbed INTEGER NOT NULL REFERENCES hire(agent),
    at INTEGER NOT NULL,
    by TEXT NOT NULL,
    reason TEXT NOT NULL,
    merge_commit TEXT,
    clashes TEXT CHECK (clashes IS NULL OR json_valid(clashes)),
    split_at INTEGER,
    split_by TEXT,
    split_commit TEXT
) STRICT;
CREATE INDEX hire_merge_kept ON hire_merge(kept);
CREATE UNIQUE INDEX hire_merge_absorbed ON hire_merge(absorbed)
    WHERE split_at IS NULL;

-- Tasks and spend

-- A task (H12): what a chief's line, a retry, a reflection, an errand,
-- a dream or a worker starts. `payer` is the hamlet that pays, or the
-- realm when empty. `chief` is the chief overseeing it.
CREATE TABLE task (
    id INTEGER PRIMARY KEY,
    origin TEXT NOT NULL CHECK (origin IN
        ('chief', 'retry', 'reflection', 'errand', 'dream', 'worker',
         'notice')),
    agent INTEGER NOT NULL REFERENCES agent(id),
    chief INTEGER REFERENCES chief(id),
    payer TEXT REFERENCES hamlet(id),
    at INTEGER NOT NULL,
    text TEXT,
    messages INTEGER NOT NULL DEFAULT 0 CHECK (messages >= 0),
    done_at INTEGER
) STRICT;
CREATE INDEX task_agent ON task(agent, at);

-- One model call, admitted once (S20), and who it is charged to.
CREATE TABLE attempt (
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    conversation INTEGER NOT NULL REFERENCES conversation(id),
    agent INTEGER NOT NULL REFERENCES agent(id),
    payer TEXT REFERENCES hamlet(id),
    task INTEGER REFERENCES task(id),
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    price TEXT NOT NULL CHECK (json_valid(price)),
    at INTEGER NOT NULL,
    PRIMARY KEY (runtime, key)
) STRICT, WITHOUT ROWID;
CREATE INDEX attempt_agent ON attempt(agent, at);
CREATE INDEX attempt_payer ON attempt(payer, at);
CREATE INDEX attempt_task ON attempt(task);

-- What an attempt used and cost, charged once.
CREATE TABLE usage (
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    ordinal INTEGER NOT NULL UNIQUE,
    input INTEGER NOT NULL CHECK (input >= 0),
    output INTEGER NOT NULL CHECK (output >= 0),
    cache_read INTEGER NOT NULL CHECK (cache_read >= 0),
    cache_write INTEGER NOT NULL CHECK (cache_write >= 0),
    micros INTEGER NOT NULL CHECK (micros >= 0),
    ms INTEGER NOT NULL CHECK (ms >= 0),
    estimated INTEGER NOT NULL CHECK (estimated IN (0, 1)),
    detail TEXT NOT NULL CHECK (json_valid(detail)),
    at INTEGER NOT NULL,
    PRIMARY KEY (runtime, key),
    FOREIGN KEY (runtime, key) REFERENCES attempt(runtime, key)
) STRICT, WITHOUT ROWID;

-- A chief starting a budget's period again: a hamlet's, or the
-- realm's when empty. Spend after the last usage and road charge it
-- names counts.
CREATE TABLE budget_reset (
    id INTEGER PRIMARY KEY,
    hamlet TEXT REFERENCES hamlet(id),
    at INTEGER NOT NULL,
    chief INTEGER NOT NULL REFERENCES chief(id),
    usage INTEGER NOT NULL,
    road_charge INTEGER NOT NULL
) STRICT;

-- Conversations

-- A running harness, with its own store under harnesses/<id>/.
CREATE TABLE runtime (
    id TEXT PRIMARY KEY,
    harness TEXT NOT NULL,
    adapter TEXT,
    version TEXT,
    begun INTEGER NOT NULL DEFAULT 0 CHECK (begun IN (0, 1))
) STRICT, WITHOUT ROWID;

-- A conversation of an agent, bound to the runtime it began in (S17).
-- `handle` is the runtime's own id for it. A chat session has its
-- chief; letters and questions their sender as `peer`; an errand its
-- kind, its owner and its instructions.
CREATE TABLE conversation (
    id INTEGER PRIMARY KEY,
    agent INTEGER NOT NULL REFERENCES agent(id),
    kind TEXT NOT NULL CHECK (kind IN
        ('chat', 'letters', 'questions', 'worker', 'errand', 'dream')),
    chief INTEGER REFERENCES chief(id),
    session INTEGER,
    peer INTEGER REFERENCES agent(id),
    errand TEXT,
    owner TEXT,
    instructions TEXT,
    runtime TEXT NOT NULL REFERENCES runtime(id),
    handle TEXT,
    adapter TEXT,
    version TEXT,
    model TEXT NOT NULL CHECK (json_valid(model)),
    fork_of INTEGER REFERENCES conversation(id),
    fork_at TEXT,
    previous INTEGER REFERENCES conversation(id),
    tools TEXT CHECK (tools IS NULL OR json_valid(tools)),
    tools_told TEXT,
    cancelling INTEGER NOT NULL DEFAULT 0 CHECK (cancelling IN (0, 1)),
    -- A chief stopped its running turn, whose end is then no failure.
    stopped INTEGER NOT NULL DEFAULT 0 CHECK (stopped IN (0, 1)),
    last TEXT CHECK (last IS NULL OR last IN ('done', 'failed', 'stopped')),
    unreflected INTEGER NOT NULL DEFAULT 0 CHECK (unreflected >= 0),
    retiring INTEGER NOT NULL DEFAULT 0 CHECK (retiring IN (0, 1)),
    entered TEXT,
    last_input TEXT,
    made INTEGER NOT NULL,
    UNIQUE (runtime, handle),
    CHECK ((kind = 'chat') = (chief IS NOT NULL AND session IS NOT NULL)),
    CHECK ((kind IN ('letters', 'questions')) = (peer IS NOT NULL)),
    CHECK ((kind = 'errand') = (errand IS NOT NULL))
) STRICT;
CREATE INDEX conversation_agent ON conversation(agent);
CREATE UNIQUE INDEX conversation_peer ON conversation(agent, kind, peer)
    WHERE peer IS NOT NULL;
CREATE UNIQUE INDEX conversation_errand ON conversation(errand, owner)
    WHERE errand IS NOT NULL;

-- A chief's chat with an agent, and its current session.
CREATE TABLE chat (
    agent INTEGER NOT NULL REFERENCES agent(id),
    chief INTEGER NOT NULL REFERENCES chief(id),
    current INTEGER NOT NULL CHECK (current >= 1),
    reflections INTEGER NOT NULL DEFAULT 0 CHECK (reflections >= 0),
    PRIMARY KEY (agent, chief)
) STRICT, WITHOUT ROWID;

-- The conversation a chat's session stands for now. A rewind points
-- the session at its fork.
CREATE TABLE chat_session (
    agent INTEGER NOT NULL,
    chief INTEGER NOT NULL,
    session INTEGER NOT NULL CHECK (session >= 1),
    conversation INTEGER NOT NULL UNIQUE REFERENCES conversation(id),
    began INTEGER NOT NULL,
    PRIMARY KEY (agent, chief, session),
    FOREIGN KEY (agent, chief) REFERENCES chat(agent, chief)
) STRICT, WITHOUT ROWID;

-- An input to a conversation, taken in once and in order (S23).
-- `request` is the server's id for it, `submission` the runtime's.
CREATE TABLE input (
    conversation INTEGER NOT NULL REFERENCES conversation(id),
    request TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN
        ('chief', 'retry', 'reflection', 'notice', 'letter', 'question',
         'reply', 'errand', 'worker', 'dream')),
    task INTEGER REFERENCES task(id),
    content TEXT NOT NULL,
    mark TEXT NOT NULL CHECK (json_valid(mark)),
    steer INTEGER NOT NULL CHECK (steer IN (0, 1)),
    boundary INTEGER NOT NULL DEFAULT 0 CHECK (boundary IN (0, 1)),
    state TEXT NOT NULL CHECK (state IN ('kept', 'admitted', 'dropped')),
    submission TEXT,
    status TEXT CHECK (status IS NULL OR status IN
        ('queued', 'placed', 'done', 'unanswered')),
    reason TEXT,
    detail TEXT CHECK (detail IS NULL OR json_valid(detail)),
    followed INTEGER NOT NULL DEFAULT 0 CHECK (followed IN (0, 1)),
    PRIMARY KEY (conversation, request),
    UNIQUE (conversation, ordinal)
) STRICT;
CREATE INDEX input_task ON input(task);

-- A transcript entry a runtime kept, as the server keeps it for
-- clients.
CREATE TABLE item (
    conversation INTEGER NOT NULL REFERENCES conversation(id),
    entry TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    body TEXT NOT NULL CHECK (json_valid(body)),
    PRIMARY KEY (conversation, entry),
    UNIQUE (conversation, ordinal)
) STRICT;

-- A reflection asked of a chat (C5), with the task it runs under.
CREATE TABLE reflection (
    conversation INTEGER NOT NULL REFERENCES conversation(id),
    request TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    count INTEGER NOT NULL CHECK (count >= 0),
    task INTEGER NOT NULL REFERENCES task(id),
    at INTEGER NOT NULL,
    PRIMARY KEY (conversation, request)
) STRICT, WITHOUT ROWID;

-- A final answer that failed its check, and whether the chief was
-- told.
CREATE TABLE answer_fault (
    conversation INTEGER NOT NULL REFERENCES conversation(id),
    input TEXT NOT NULL,
    words TEXT NOT NULL,
    reported INTEGER NOT NULL DEFAULT 0 CHECK (reported IN (0, 1)),
    PRIMARY KEY (conversation, input)
) STRICT, WITHOUT ROWID;

-- The society

-- A letter or question one agent sent another along a road (H30),
-- once per tool call.
CREATE TABLE message (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('letter', 'question')),
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    sender INTEGER NOT NULL REFERENCES agent(id),
    receiver INTEGER NOT NULL REFERENCES agent(id),
    from_conversation INTEGER NOT NULL REFERENCES conversation(id),
    to_conversation INTEGER NOT NULL REFERENCES conversation(id),
    address TEXT NOT NULL,
    task INTEGER NOT NULL REFERENCES task(id),
    text TEXT NOT NULL,
    at INTEGER NOT NULL,
    input TEXT,
    answer TEXT,
    failure TEXT,
    carried INTEGER NOT NULL DEFAULT 0 CHECK (carried IN (0, 1)),
    replied INTEGER NOT NULL DEFAULT 0 CHECK (replied IN (0, 1)),
    refused INTEGER NOT NULL DEFAULT 0 CHECK (refused IN (0, 1)),
    UNIQUE (runtime, key)
) STRICT;
CREATE INDEX message_pair ON message(sender, receiver, at);
CREATE INDEX message_receiver ON message(receiver, at);

-- An objection to a chief's instruction (H10), one per task: by the
-- instructed agent, or by its lead's review.
CREATE TABLE objection (
    task INTEGER PRIMARY KEY REFERENCES task(id),
    agent INTEGER NOT NULL REFERENCES agent(id),
    source TEXT NOT NULL CHECK (source IN ('agent', 'review')),
    reason TEXT NOT NULL,
    wait INTEGER REFERENCES wait(id),
    at INTEGER NOT NULL,
    ruling TEXT CHECK (ruling IS NULL OR ruling IN ('go', 'withdraw')),
    ruled_at INTEGER
) STRICT;

-- A lead's review of a chief's instruction to a hire under it.
CREATE TABLE review (
    task INTEGER PRIMARY KEY REFERENCES task(id),
    lead INTEGER NOT NULL REFERENCES agent(id),
    conversation INTEGER REFERENCES conversation(id),
    outcome TEXT CHECK (outcome IS NULL OR outcome IN
        ('pass', 'object', 'skipped', 'failed')),
    at INTEGER NOT NULL
) STRICT;

-- A skill offered to the library. `tree` is the offered folder, kept
-- as a tree in the library's repository.
CREATE TABLE skill_offer (
    id INTEGER PRIMARY KEY,
    agent INTEGER NOT NULL REFERENCES agent(id),
    skill TEXT NOT NULL,
    reason TEXT NOT NULL,
    task INTEGER REFERENCES task(id),
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    tree TEXT,
    verdict TEXT CHECK (verdict IS NULL OR verdict IN
        ('accepted', 'refused', 'failed')),
    words TEXT,
    at INTEGER NOT NULL,
    UNIQUE (runtime, key)
) STRICT;

-- What came to and left the library.
CREATE TABLE library_event (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN
        ('starter', 'shelved', 'refused', 'unshelved')),
    skill TEXT NOT NULL,
    agent INTEGER REFERENCES agent(id),
    chief INTEGER REFERENCES chief(id),
    at INTEGER NOT NULL,
    reason TEXT,
    commit_id TEXT
) STRICT;

-- A dream: a hire's, which brings what it learned home, or a
-- scholar's, which tidies its `main`. `ending` marks the last dream of
-- a hire that ends.
CREATE TABLE dream (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('hire', 'scholar')),
    scholar INTEGER NOT NULL REFERENCES scholar(id),
    hire INTEGER REFERENCES hire(agent),
    ending INTEGER NOT NULL DEFAULT 0 CHECK (ending IN (0, 1)),
    conversation INTEGER REFERENCES conversation(id),
    task INTEGER REFERENCES task(id),
    began INTEGER NOT NULL,
    ended INTEGER,
    outcome TEXT CHECK (outcome IS NULL OR outcome IN
        ('done', 'nothing', 'failed', 'stopped')),
    main_commit TEXT,
    branch_commit TEXT,
    CHECK ((kind = 'hire') = (hire IS NOT NULL))
) STRICT;
CREATE INDEX dream_scholar ON dream(scholar, began);

-- Oversight

-- What waits for a chief (H11, H10, H34). `conversation` is the chat
-- it is asked in.
CREATE TABLE wait (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN
        ('approval', 'fault', 'question', 'objection', 'newcomer')),
    danger TEXT CHECK (danger IS NULL OR danger IN
        ('network', 'unconfined', 'reserved', 'edit', 'command')),
    agent INTEGER REFERENCES agent(id),
    -- The newcomer a `newcomer` wait asks about.
    scholar INTEGER REFERENCES scholar(id),
    conversation INTEGER REFERENCES conversation(id),
    task INTEGER REFERENCES task(id),
    about TEXT NOT NULL,
    change TEXT CHECK (change IS NULL OR json_valid(change)),
    answers TEXT NOT NULL CHECK (json_valid(answers)),
    questions TEXT CHECK (questions IS NULL OR json_valid(questions)),
    since INTEGER NOT NULL,
    asked INTEGER NOT NULL DEFAULT 1 CHECK (asked >= 1),
    open INTEGER NOT NULL DEFAULT 1 CHECK (open IN (0, 1)),
    how TEXT,
    text TEXT,
    chief INTEGER REFERENCES chief(id),
    settled INTEGER
) STRICT;
CREATE INDEX wait_open ON wait(open, agent);

-- The gate's decision on one tool call, by the runtime's key for it.
CREATE TABLE tool_call (
    runtime TEXT NOT NULL,
    key TEXT NOT NULL,
    conversation INTEGER NOT NULL REFERENCES conversation(id),
    digest BLOB NOT NULL,
    input TEXT,
    tool TEXT NOT NULL,
    net INTEGER CHECK (net IS NULL OR net IN (0, 1)),
    block TEXT,
    wait INTEGER REFERENCES wait(id),
    how TEXT,
    text TEXT,
    change TEXT CHECK (change IS NULL OR json_valid(change)),
    PRIMARY KEY (runtime, key)
) STRICT, WITHOUT ROWID;
CREATE INDEX tool_call_wait ON tool_call(wait);

-- A turn's refusals and questions so far, for its caps.
CREATE TABLE turn_gate (
    conversation INTEGER PRIMARY KEY REFERENCES conversation(id),
    input TEXT,
    strikes INTEGER NOT NULL DEFAULT 0 CHECK (strikes >= 0),
    questions INTEGER NOT NULL DEFAULT 0 CHECK (questions >= 0),
    refused TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(refused))
) STRICT;

-- Edits and commands a chief allowed for the rest of a chat session.
CREATE TABLE trust (
    agent INTEGER NOT NULL REFERENCES agent(id),
    chief INTEGER NOT NULL REFERENCES chief(id),
    session INTEGER NOT NULL CHECK (session >= 1),
    edit INTEGER NOT NULL DEFAULT 0 CHECK (edit IN (0, 1)),
    command INTEGER NOT NULL DEFAULT 0 CHECK (command IN (0, 1)),
    PRIMARY KEY (agent, chief, session)
) STRICT, WITHOUT ROWID;

-- A lasting permission a chief gave an agent, until taken back or the
-- agent ends.
CREATE TABLE permission_grant (
    id INTEGER PRIMARY KEY,
    agent INTEGER NOT NULL REFERENCES agent(id),
    permission TEXT NOT NULL CHECK (permission IN ('network', 'unconfined')),
    at INTEGER NOT NULL,
    chief INTEGER NOT NULL REFERENCES chief(id),
    revoked_at INTEGER,
    revoked_by TEXT
) STRICT;
CREATE UNIQUE INDEX permission_grant_open
    ON permission_grant(agent, permission) WHERE revoked_at IS NULL;

-- Done once

-- The outcome of a request kept under its identity: a tool call by
-- its runtime and key, a client request by its chief and id. An empty
-- outcome is a request begun.
CREATE TABLE marker (
    source TEXT NOT NULL,
    key TEXT NOT NULL,
    kind TEXT NOT NULL,
    digest BLOB NOT NULL,
    outcome TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (source, key)
) STRICT, WITHOUT ROWID;

-- An effect kept with the change that needs it and run after the
-- commit, again after a crash, until done. A key keeps two of one
-- effect from waiting at once.
CREATE TABLE job (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    key TEXT,
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    made INTEGER NOT NULL,
    after INTEGER,
    tries INTEGER NOT NULL DEFAULT 0 CHECK (tries >= 0),
    failure TEXT,
    done INTEGER
) STRICT;
CREATE UNIQUE INDEX job_waiting ON job(key)
    WHERE done IS NULL AND key IS NOT NULL;
CREATE INDEX job_due ON job(done, after);
