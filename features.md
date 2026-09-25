# Luminatti: the semantic change sidecar for coding agents

> Product strategy and feature roadmap  
> Reimagined: 1 September 2026  
> Core mission: run beside a coding agent, continuously show what it is doing and changing, explain the semantic meaning and impact, detect drift and risk, and give the human safe points to intervene.

## Executive decision

Luminatti should not primarily become another general-purpose file comparison application.

It should become the **independent observation and control plane for agent-written code**.

Araxis Merge and Beyond Compare remain useful benchmarks for deterministic diff quality, navigation, three-way merge, repeatability, and auditability. Those capabilities are supporting infrastructure. The main competitor is the experience developers have today when an agent changes a repository: scrolling through terminal output, periodically running `git diff`, guessing which changes belong to which step, and discovering scope drift or broken assumptions after the work is already complete.

Luminatti's product loop should be:

```text
Agent starts work
      ↓
Luminatti captures intent, baseline, actions, and repository events
      ↓
Luminatti correlates actions into live change sets
      ↓
Luminatti explains structural and semantic meaning with evidence
      ↓
Luminatti checks scope, risk, tests, policies, and unresolved questions
      ↓
Human watches, asks, annotates, approves, redirects, or rolls back
      ↓
Agent responds and Luminatti verifies the resulting change
```

The product promise becomes:

> **See what your coding agent is really doing—while it is doing it.**

## Product thesis

Coding agents create a new software-development problem. Producing code is becoming cheap; maintaining comprehension, control, and accountability is not.

Traditional diff tools answer:

- What text differs between A and B?
- Which side should win during a merge?

Luminatti should answer a broader set of questions:

- What is the agent doing right now?
- Which user request or plan step caused this change?
- What changed semantically, not just textually?
- Is the agent still within the requested scope?
- What behaviour, public API, data, security, or operational assumptions changed?
- What did the agent validate, and what remains unverified?
- Which changes are mechanical, generated, speculative, risky, or unrelated?
- Can I safely pause, redirect, reject, or restore part of the work?
- Did the agent actually address my review feedback?
- Can another person replay and audit the session later?

This makes Luminatti an **agent change intelligence system**, with diff and merge as foundational primitives.

## The category Luminatti should create

Possible category names:

- Agent change sidecar.
- Agent observability for code.
- Semantic review cockpit.
- Human control plane for coding agents.

The clearest description is:

> Luminatti runs alongside any coding agent and turns its ongoing activity into an evidence-backed, semantically organised review that a human can understand and control.

### What Luminatti is

- Independent of the agent's own self-report.
- Live rather than end-of-task only.
- Repository- and language-aware.
- Deterministic at the evidence layer.
- Semantic at the interpretation layer.
- Human-controlled at every consequential intervention.
- Useful with no AI provider, stronger when a semantic model is enabled.
- Agent-agnostic through observation, with deeper integrations through adapters.

### What Luminatti is not

- Another autonomous coding agent.
- A chat transcript viewer.
- A prettier `git diff` that opens only after work finishes.
- A monitoring product that records everything but cannot explain it.
- A security system that claims certainty from probabilistic inference.
- A general folder synchroniser or remote backup client.
- A mechanism that silently rewrites an agent's work.

## Primary users and jobs

### Developer pairing with one agent

The developer wants to delegate implementation while retaining awareness. They need a low-noise feed, semantic summaries, scope-drift warnings, review checkpoints, and a quick way to send precise feedback.

### Developer supervising long-running work

The developer is not watching every command. They need a trustworthy replay, milestone summaries, unresolved risks, validation status, and notification only when attention is valuable.

### Developer supervising multiple agents

The developer needs attribution, overlapping-file detection, conflicting assumptions, branch/worktree relationships, dependency ordering, and a combined semantic view.

### Reviewer receiving agent-generated work

The reviewer needs to understand intent and behavioural effect without reading the entire transcript. They need evidence, uncertainty, test coverage, scope boundaries, and a durable review bundle.

### Engineering lead or regulated team

The team needs policies, provenance, approval gates, reproducibility, private/local operation, and an audit trail showing what the agent did and what a human approved.

## Product principles

### Independent evidence before interpretation

Filesystem, Git/Jujutsu, compiler, test, and static-analysis evidence are the source of truth. Agent messages and model explanations are claims that must be linked to that evidence.

### Live comprehension, not live noise

Do not stream every file-write event as a notification. Correlate low-level activity into meaningful episodes such as “renamed authentication type,” “updated six call sites,” or “generated snapshots after test change.”

### Raw truth is always reachable

Every semantic statement links back through structure, diff hunks, source bytes, event timestamps, and source hashes. Semantic views never replace or hide the underlying diff.

### Observation and control are separate permissions

Luminatti can always operate read-only. Pausing an agent, rejecting an edit, restoring files, running checks, or sending feedback requires an explicit capability and user action.

### No fake causality

A filesystem watcher can prove that a file changed after an event; it cannot always prove which agent command caused it. Luminatti must label direct evidence, correlation, heuristic inference, model inference, and agent self-report differently.

### Safe interruption over silent automation

When risk is high, Luminatti should ask for attention, create a checkpoint, or prepare a reversible action. It should not silently “fix” the agent or mutate source based on a semantic guess.

### Local-first and provider-neutral

The deterministic observer works without sending repository content anywhere. Semantic providers are optional, inspectable, scoped, and selected by the user or organisation.

## Operating modes

Luminatti cannot obtain the same visibility or control from every agent. The product should make integration depth explicit.

| Mode | How it works | What Luminatti can know | What Luminatti can control |
|---|---|---|---|
| Observe | `luminatti watch` monitors repository, VCS, processes it owns, and validation results | File/VCS changes, timing, snapshots, tests Luminatti runs; causality is correlated | Nothing outside Luminatti; read-only review and alerts |
| Wrapped | `luminatti run -- <agent command>` launches the agent inside a supervised session | Observe-mode evidence plus process lifecycle, terminal stream, checkpoints, and signals | Pause/continue/terminate the wrapped process, subject to OS support and explicit user action |
| Connected | Agent hook/plugin/MCP/telemetry sends plan, tool, prompt, patch, and status events | Exact agent-declared intent and action IDs alongside independent evidence | Send structured feedback; request pause/checkpoint if the agent supports it |
| Brokered | The agent proposes edits through Luminatti's patch/workspace API | Exact proposed patch before commit, direct causality, policy evaluation before write | Approve, reject, edit, queue, or apply a proposed change transactionally |

The UI should always display the active mode and confidence boundary. It must never show “blocked” or “prevented” when Luminatti only observed a change after it happened.

## Core user experience

### Start a session

```text
luminatti watch --agent codex
luminatti run -- claude
luminatti watch --session "Add OAuth device flow" --intent task.md
```

Luminatti records:

- Repository and worktree identity.
- Baseline commit, index, working tree, untracked files, and pre-existing user changes.
- User intent and explicit constraints.
- Agent identity/integration mode.
- Allowed paths, denied paths, budgets, and required checks.
- Environment facts needed for reproducibility, excluding secrets.

Pre-existing changes are marked as protected context, not attributed to the agent.

### Watch the work

The default view is a quiet live cockpit:

```text
Session: Add OAuth device flow              Mode: Connected
Agent: Codex                                Elapsed: 18m

Now      Updating token polling and retry behaviour
Scope    7 intended files · 1 unexpected file
Meaning  + device-code state machine
         ~ refresh-token API signature
         ~ retry changes from fixed → exponential
Risk     HIGH: auth behaviour · MEDIUM: public API
Checks   unit 42/42 ✓ · integration not run · typecheck running
Review   3 clusters reviewed · 2 need attention
```

The developer can drill from intent cluster → symbol → hunk → raw bytes → originating event.

### Intervene

Depending on integration mode, the user can:

- Annotate a line, symbol, intent cluster, plan step, or session.
- Ask “why did this change?” with evidence-linked answers.
- Send structured feedback to the agent.
- Request or create a checkpoint.
- Mark a change as expected, unrelated, rejected, or requiring follow-up.
- Pause a wrapped/connected agent.
- Approve or reject a brokered patch.
- Restore an agent-owned change to a checkpoint without touching protected pre-existing work.
- Require checks before the agent continues or finishes.

### Finish the session

Luminatti produces a concise hand-off:

- Requested outcome versus observed result.
- Semantic change clusters.
- Scope deviations and accepted exceptions.
- Public API, data, security, dependency, and operational changes.
- Validation performed, failed, skipped, or still running.
- Human decisions and unresolved review items.
- Final patch and source hashes.
- Provenance and confidence for semantic claims.

## The twelve defining features

If only twelve areas are funded, build these in order.

| Order | Feature | Product outcome |
|---:|---|---|
| 1 | Agent session boundary and protected baseline | Luminatti knows what existed before the agent and never misattributes user work |
| 2 | Universal live event and snapshot engine | Changes appear continuously and survive agent restarts or crashes |
| 3 | Intent contract and scope model | The requested outcome becomes something Luminatti can evaluate against observed work |
| 4 | Change episode correlation and causal timeline | Noisy file events become understandable units of work |
| 5 | Stable semantic change graph | Every view shares durable files, symbols, hunks, events, claims, and evidence |
| 6 | Structural diff and move/refactor detection | Luminatti explains code changes beyond added and removed lines without requiring an LLM |
| 7 | Live scope, risk, and blast-radius radar | The human knows when attention is valuable while the agent is still working |
| 8 | Validation intelligence | Tests, builds, types, lint, and coverage are mapped to the changes they support |
| 9 | Evidence-backed semantic reviewer | Optional AI explains intent and risk without becoming the source of truth |
| 10 | Checkpoints, time machine, and safe partial rollback | The human can recover or compare agent states without erasing unrelated work |
| 11 | Closed-loop feedback verification | Luminatti tracks whether the agent actually addressed each review request |
| 12 | Policy gates and brokered edits | Teams can move from observation to enforceable, review-before-write control |

## Current Luminatti strengths to build on

| Existing capability | Role in the new mission |
|---|---|
| Fast Rust TUI and early native macOS workspace | Always-on cockpit surfaces |
| Side-by-side diff and word-level emphasis | Raw evidence view beneath semantic interpretation |
| Tree-sitter highlighting and sticky AST context | Starting point for structural entities and symbol anchoring |
| Git and Jujutsu backends | Baselines, checkpoints, attribution, conflict state, and session completion |
| Working-tree/commit/range/stacked views | State history and time-machine views |
| GitHub PR viewed state | External review integration |
| Watch mode | Earliest form of live agent observation |
| Selection/hunk/file annotations | Human feedback primitives |
| Annotation export to coding agents | Early closed-loop agent integration |
| Fuzzy global search with preview | Cross-session evidence navigation |
| Multi-provider AI explanation | Provider foundation for a future semantic reviewer |

The product already contains pieces of the sidecar. The important change is to reorganise them around a durable agent session and semantic event model.

## Competitive reframing

Araxis Merge and Beyond Compare are comparison suites. Their table-stakes features still teach Luminatti important lessons:

- Accurate, configurable comparison.
- Clear important/unimportant/ignored distinctions.
- Three-way reconciliation.
- Saved sessions and repeatability.
- Portable reports and automation.
- Safe editing, undo, and conflict visibility.

Luminatti should implement the subset needed for trustworthy agent review, but should not chase image, registry, media, remote-folder, or office-document breadth before its agent experience is excellent.

| Capability | Araxis / Beyond Compare | Luminatti opportunity |
|---|---|---|
| Compare final states | Excellent | Compare continuously across the agent's session |
| Text and folder diff | Excellent | Connect diffs to intent, actions, symbols, tests, and risk |
| Three-way merge | Excellent | Resolve agent-created conflicts with causal and semantic context |
| Saved session | Comparison settings and sources | Full replay of intent, actions, changes, decisions, and validation |
| Ignore rules | Powerful but manually configured | Rules plus structural/semantic noise classification with explanations |
| Reports | Static comparison evidence | Agent-session audit and human-approval record |
| Automation | Scripts and command line | Agent adapters, event protocol, policy gates, and brokered patch API |
| Semantic meaning | Limited | Core differentiator |
| Agent supervision | Not core | Product category |

## Core architecture

### A-001: Agent session model — P0

Every observed task is an `AgentSession` with a stable ID.

It should record:

- Repository/worktree identity and VCS backend.
- Baseline snapshot and protected pre-existing changes.
- Agent identity, process identity, adapter, and integration mode.
- User request, clarified requirements, plan, and constraints.
- Start/end timestamps and monotonic event ordering.
- Checkpoints and derived change sets.
- Human annotations, approvals, policy decisions, and feedback.
- Validation runs and outputs.
- Privacy policy and semantic provider used.
- Final status: completed, stopped, failed, abandoned, or superseded.

Sessions must survive UI closure and agent process restarts. A session can attach to a new process without losing its historical boundary.

### A-002: Append-only evidence event log — P0

Capture facts as immutable events before deriving interpretations.

Candidate event types:

- Session and process started/stopped.
- Agent plan/step/tool events from adapters.
- File created/read/modified/renamed/deleted when observable.
- VCS index, commit, branch, rebase, and conflict changes.
- Patch proposed/applied/rejected.
- Command declared or executed through a supported integration.
- Build/test/lint/typecheck started, output, and result.
- User annotation, approval, pause, feedback, restore, and policy override.
- Snapshot/checkpoint created.
- Semantic claim created, updated, invalidated, or superseded.

Each event carries source, timestamp, source hashes where relevant, integration confidence, and links to parent/action IDs when available.

### A-003: Snapshot and incremental diff engine — P0

Luminatti needs more than a simple filesystem watcher.

- Record a baseline without modifying the working tree.
- Debounce editors that write via temporary file and rename.
- Preserve untracked and ignored-file policy.
- Incrementally re-diff only affected files and entities.
- Detect external/user changes separately where evidence permits.
- Create lightweight automatic checkpoints before risky episodes.
- Recover after missed filesystem events by reconciling current hashes.
- Handle large repositories with bounded memory and cancellable background work.
- Mark incomplete or uncertain observation explicitly.

### A-004: Agent integration protocol — P0

Define a small versioned protocol rather than one bespoke integration per agent.

Minimum messages:

```text
session.start
intent.declare
plan.update
step.start / step.finish
tool.start / tool.finish
patch.propose / patch.applied
validation.start / validation.finish
agent.message
attention.request
session.finish
```

Transports may include stdio JSONL, local socket, hooks, MCP, OpenTelemetry-style events, or a native plugin. All connected-agent events remain agent claims until correlated with independent repository evidence.

### A-005: Semantic change graph — P0 architecture

Semantic understanding should be a core data model from the start, even if early producers are deterministic.

Node types:

- Session, intent, requirement, constraint, and plan step.
- Event, command, patch, checkpoint, and validation run.
- Repository, file, directory, configuration key, dependency, schema, and migration.
- Symbol, AST node, API endpoint, database entity, test, and generated artefact.
- Diff hunk, changed span, conflict, annotation, claim, risk, and decision.

Important edges:

- `implements`, `caused-by`, `observed-after`, `modifies`, `moves-from`.
- `calls`, `imports`, `overrides`, `exposes`, `persists-to`, `tests`.
- `validates`, `contradicts`, `supports`, `derived-from`, `supersedes`.
- `within-scope`, `violates-constraint`, `reviewed-by`, `resolved-by`.

Every graph node representing source content needs raw and normalised hashes plus stable anchors. This allows claims and annotations to become stale correctly.

### A-006: Evidence and confidence ledger — P0

Luminatti should visually distinguish:

| Evidence class | Example | UI treatment |
|---|---|---|
| Direct | Git blob hash changed; test process exited 1 | Fact |
| Declared | Connected agent says it is editing auth retries | Agent claim |
| Correlated | File changed during that tool action | Likely association |
| Deterministic analysis | Function signature changed in parsed AST | Derived fact |
| Heuristic | Two blocks appear to be a move | Confidence-labelled |
| Model inference | Change may alter token expiry behaviour | Semantic claim with evidence and confidence |
| Human decision | Reviewer accepted scope exception | Authoritative decision |

No semantic summary should flatten these into one confident narrative.

### A-007: Capability and permission model — P0

Permissions should be explicit per session:

- Observe repository.
- Read agent telemetry.
- Run approved validations.
- Send feedback.
- Signal/pause/terminate wrapped process.
- Create VCS checkpoint.
- Restore agent-owned changes.
- Apply brokered patch.
- Access network semantic provider.

The UI should display which control actions are real in the current mode. An agent adapter cannot grant itself more Luminatti permissions.

## Phase 1: live agent observability

### O-001: Protected baseline and ownership map — P0

Before observation starts, Luminatti inventories:

- HEAD/base revision.
- Staged, unstaged, untracked, ignored, and conflicted files.
- Existing file hashes and optional content snapshots.
- Active branch/worktree and relevant sibling worktrees.

Changes are classified as pre-existing, observed during session, definitely agent-originated, probably agent-originated, user-originated, external, or unknown. Restore/discard operations may target only attributable agent changes by default.

This prevents the catastrophic mistake of rolling back the user's own dirty work.

### O-002: Intent contract — P0

Convert the user's request into a reviewable contract, not a hidden model prompt.

Contract fields:

- Desired outcome and acceptance criteria.
- Explicit non-goals.
- Allowed and protected paths.
- Expected subsystems and public interfaces.
- Change, time, command, network, or cost budgets.
- Required validation.
- Human approval gates.
- Known uncertainties and questions.

The user can write this directly, import it from the agent task, or accept a suggested extraction. Model-extracted requirements are drafts until the user or connected agent confirms them.

### O-003: Live activity timeline — P0

Create one ordered, filterable timeline combining:

- Agent plan and messages.
- Tool/command episodes.
- File/VCS changes.
- Semantic change episodes.
- Test/build results.
- Warnings, approvals, feedback, and checkpoints.

The timeline should default to semantic episodes, with raw events available on expansion. Users can scrub to any point and see repository state relative to baseline and final state.

### O-004: Change episode correlation — P0

Group bursts of events into meaningful episodes using direct action IDs, time windows, file overlap, symbols, plan steps, and validation boundaries.

Examples:

- “Introduced `DeviceCodePoller` and three tests.”
- “Renamed `TokenResult` to `OAuthTokenResult` across nine files.”
- “Regenerated lockfile after adding dependency.”
- “Reverted first retry implementation after test failure.”

Users can split, merge, retitle, or reassign an episode. Manual corrections are durable and become evaluation data, not hidden training data.

### O-005: Live semantic delta panel — P0/P1

For every active episode, show layers:

1. Raw byte/text changes.
2. Normalised text changes.
3. Structural changes.
4. Behavioural/semantic claims.
5. Impact and validation state.

Users can move between layers without losing their position. The semantic layer may say “unknown” or “insufficient evidence”; that is a valid result.

### O-006: Scope drift radar — P0

Continuously compare observed changes with the intent contract.

Signals:

- Changed path outside allowed scope.
- New dependency, API, migration, configuration, or generated file not anticipated.
- Change count or churn budget exceeded.
- Agent begins an unrelated refactor.
- Required file/test remains untouched.
- Agent modifies protected pre-existing work.
- Plan step changes after implementation starts.

Warnings should include the rule, evidence, likely explanation, and actions: accept exception, ask agent, pause, annotate, or ignore once.

### O-007: Agent-aware notifications — P0

Notify only when action is useful:

- Scope or policy boundary crossed.
- Risk classification increases materially.
- Agent requests a decision.
- Destructive command or schema migration is proposed.
- Tests repeatedly fail or become flaky.
- Agent stalls, loops, or repeatedly edits/reverts the same area.
- Required check finishes.
- Session appears complete but acceptance criteria remain unmet.

Support quiet mode, risk thresholds, batching, and “notify at next safe checkpoint.”

### O-008: “Why did this change?” — P1

From a file, symbol, or hunk, reconstruct:

- Earliest event that introduced it.
- Agent-declared plan step or tool action.
- Related changes in the same episode.
- Relevant user request or feedback.
- Subsequent edits/reverts.
- Tests or checks affected.
- Confidence of the causal connection.

Without a connected integration, Luminatti must answer “changed during episode X” rather than invent a command-level cause.

### O-009: Session replay and time machine — P1

Replay the work semantically or at raw-event granularity:

- Scrub through automatic and manual checkpoints.
- Compare any two times.
- Follow one symbol or file through the session.
- Show changes introduced and later reverted.
- Show what the agent knew or declared at each point when telemetry exists.
- Fork a read-only comparison from any checkpoint.

The replay archive should support references-only, changed-content, or self-contained privacy modes.

## Phase 2: semantic comprehension

### S-001: Deterministic structural diff — P0/P1

Use tree-sitter and language tooling to classify:

- Function/type/module additions and deletions.
- Signature, visibility, annotation, field, and body changes.
- Symbol rename, move, copy, extraction, and inlining candidates.
- Import/dependency changes.
- Formatting-only, comment-only, and generated-only edits.
- Parse failures and text fallback.

Structural alignment should reduce diff noise, not hide content. Each matched entity links to exact raw spans on both sides.

### S-002: Intent-based change clusters — P1

Group the session into coherent outcomes across files:

- Domain implementation.
- API surface.
- Persistence or migration.
- Tests and fixtures.
- Documentation.
- Generated artefacts and dependency metadata.

Each cluster shows its relationship to the intent contract, evidence, confidence, risk, validation, and review status. Users can manually regroup changes.

### S-003: Semantic meaning cards — P1

Produce concise, falsifiable claims such as:

- “Polling now backs off exponentially instead of every five seconds.”
- “The public constructor gained a required `Clock` dependency.”
- “Expired device codes now return a typed error rather than `None`.”
- “The migration makes `provider_id` unique; duplicate existing rows may fail.”

Every card includes:

- Claim and confidence.
- Exact supporting hunks/symbols.
- Contradicting or missing evidence.
- Potential consumers/impact.
- Validation status.
- Provider/model/analyser provenance.
- Stale state when source changes.

### S-004: Blast-radius graph — P1

Estimate affected behaviour using deterministic references first and model interpretation second:

- Direct callers and implementers.
- Public/exported API.
- Tests covering the changed entity.
- Configuration, migrations, schemas, generated clients, and docs.
- Cross-language or runtime boundaries.
- Deployment and operational files.
- Unchanged consumers that may now be incompatible.

The graph must distinguish “referenced by” from “behaviourally impacted.”

### S-005: Risk radar — P1

Classify attention areas, not correctness:

- Authentication/authorisation and secrets.
- Destructive filesystem or database operations.
- Data migration and backwards compatibility.
- Public API or protocol changes.
- Concurrency, retries, timeouts, and error handling.
- Dependency and build-chain changes.
- Privilege, network, and external-service behaviour.
- Large untested or generated changes.
- Removal of checks, logging, or validation.

Risk rules are visible and project-configurable. Model-added risks are labelled as inferences.

### S-006: Semantic noise separation — P1

Allow users to de-emphasise without losing truth:

- Formatting and line-ending churn.
- Pure symbol moves/renames.
- Generated output explained by a source change.
- Lockfile updates explained by declared dependency changes.
- Snapshot updates explained by test behaviour changes.
- Repeated mechanical edits.

A “show only residual meaning” lens displays edits not accounted for by the detected mechanical transformation. Raw changes remain one keystroke away.

### S-007: Natural-language review queries — P1

Examples:

- “What behaviour changed since the last passing checkpoint?”
- “Show edits unrelated to OAuth device flow.”
- “Did the agent weaken any validation?”
- “Which public APIs changed without tests?”
- “Why was this dependency added?”
- “What did the agent try and later undo?”
- “What remains uncertain?”

The answer must show an inspectable query plan and evidence. Retrieval scopes can include current episode, unreviewed changes, session, branch, or selected subsystem.

### S-008: What is missing? — P1

Compare observed work with intent and common change obligations:

- Acceptance criterion not represented in code or tests.
- New behaviour without validation.
- API change without docs/changelog/caller update.
- Migration without compatibility or rollback consideration.
- Error path added without handling/call-site update.
- New configuration without default/example/deployment update.
- Agent claims completion while checks are missing or failing.

This feature should ask review questions rather than fabricate requirements.

## Phase 3: validation intelligence

### V-001: Unified validation feed — P0

Ingest tests, type checks, builds, linters, formatters, security scans, and custom checks from connected-agent events or commands Luminatti runs.

Record:

- Exact command, working directory, environment fingerprint with secret redaction.
- Start/end time, exit status, bounded output, and artefacts.
- Source/checkpoint hashes the result applies to.
- Whether the agent or human initiated it.
- Cached, skipped, cancelled, flaky, or superseded state.

A green test result becomes stale as soon as relevant source changes.

### V-002: Test-to-change mapping — P1

Relate validations to symbols and change clusters through coverage, test names, dependency graphs, history, or heuristic/model evidence.

Show:

- Tests directly exercising changed code.
- Changed tests without corresponding implementation changes.
- Changed behaviour with no plausible test.
- Checks that passed before a later edit and are now stale.
- Failing tests likely introduced by a specific episode.

### V-003: Differential validation planner — P1

Suggest the smallest useful checks first, then broader checks as risk grows.

The plan should be visible and editable. Running commands requires user permission or a policy grant. Luminatti records whether suggested checks were accepted, skipped, or replaced.

### V-004: Regression and loop detection — P1

Detect patterns such as:

- The same test fails repeatedly after superficially different edits.
- A file or symbol oscillates between states.
- Agent removes a failing assertion instead of fixing behaviour.
- Formatting or generation repeatedly overwrites manual changes.
- Fixing one cluster reopens an earlier resolved risk.

Offer evidence and possible interventions, not a diagnosis stated as fact.

### V-005: Completion gate — P1

Before a session is called complete, evaluate:

- Intent acceptance criteria.
- Required validations and freshness.
- Unresolved feedback and policy findings.
- Scope exceptions.
- Dirty/generated/untracked state.
- Agent-declared limitations.
- Semantic uncertainties above the configured threshold.

In observe mode this is a report. In connected mode it can request more work. In brokered mode it can withhold final patch approval.

## Phase 4: human control and feedback

### C-001: Stable semantic annotations — P0

Extend Luminatti's existing line/hunk/file annotations to target:

- Raw spans and hunks.
- Symbols and structural changes.
- Semantic claims and risk findings.
- Intent requirements and plan steps.
- Timeline episodes and validation results.

Annotations carry status, priority, category, author, source hashes, and re-anchoring evidence. They become stale rather than silently moving to the wrong code.

### C-002: Structured agent feedback inbox — P0/P1

Send the agent a machine-readable review packet:

- Instruction and desired outcome.
- Exact source/symbol anchors and hashes.
- Priority and blocking status.
- Evidence and related validation.
- Scope of permitted follow-up.

Adapters can render this into native agent messages; generic agents receive concise Markdown/stdout, preserving Luminatti's existing shell-escape workflow.

### C-003: Feedback verification loop — P1

Track each request through:

```text
Draft → Sent → Acknowledged → Change observed → Verified → Resolved
                                      ↘ Partial / Stale / Rejected
```

Luminatti should compare the requested outcome to the subsequent semantic delta. The human resolves the feedback; the agent cannot mark its own change verified.

### C-004: Checkpoints and safe restoration — P1

Create explicit and automatic checkpoints:

- Before session start.
- Before a high-risk episode or destructive command.
- Before broad formatting/generation.
- After a passing validation milestone.
- On user request.

Restoration options:

- Entire agent session.
- One episode or change cluster.
- Selected files/hunks where an inverse patch applies cleanly.
- Return to checkpoint in a new branch/worktree.

Never overwrite protected pre-existing changes. Preview the inverse operation and report conflicts before applying it.

### C-005: Pause and attention gates — P1

When supported, Luminatti can request or enforce pause at safe boundaries:

- Before applying a destructive or high-risk patch.
- On protected-path modification.
- When scope budget is exceeded.
- Before network/dependency/schema changes.
- When explicit human input is required.

Wrapped-process signals are lower fidelity than connected safe-point pauses and must be labelled accordingly.

### C-006: Brokered patch review — P2

For agents that support it, receive proposed patches before filesystem application.

- Show semantic and raw preview.
- Evaluate policies and scope.
- Approve all, reject all, edit, or approve selected transactions.
- Apply atomically with source-hash preconditions.
- Return structured rejection reasons to the agent.
- Revalidate the actual applied result.

This is the strongest control mode, but it should follow a successful read-only sidecar because it changes agent workflows.

### C-007: Counterfactual preview — P2

Compare:

- Current agent proposal.
- Previous checkpoint.
- Human-edited alternative.
- Agent's revised proposal after feedback.
- Optional structural/semantic resolution suggestion.

The user chooses an output only after seeing exact patches and validation differences.

## Phase 5: policies and teams

### P-001: Change contracts — P1

Projects define reviewable rules such as:

- Public API changes require tests and changelog entry.
- Database migrations require rollback notes and explicit approval.
- Authentication code requires security-owner review.
- Dependency additions require licence and provenance checks.
- Agent may not modify generated output directly.
- Protected paths require brokered approval.
- Session must finish with specified commands passing on the final source hash.

Rules emit findings with evidence. In observe mode they inform; in connected/brokered modes explicitly configured rules may gate progress.

### P-002: Change budgets — P1

Budget by:

- Files, lines, symbols, subsystems, or public APIs changed.
- New dependencies or migrations.
- Risk score and unresolved claims.
- Elapsed time or validation cost.
- Paths outside expected scope.

Budgets are tripwires for attention, not arbitrary quality scores.

### P-003: Multi-agent attribution lanes — P2

Show concurrent agent work as lanes sharing one repository graph.

- Per-agent sessions, worktrees, branches, tasks, and changes.
- File/symbol overlap and likely merge conflicts.
- Conflicting intent contracts or assumptions.
- Dependency ordering between tasks.
- Shared validation results and stale states.
- Human feedback and approvals per agent.

Attribution must degrade honestly when multiple agents write the same working tree without integrations.

### P-004: Cross-agent conflict forecast — P2

Predict conflicts before branches merge using file, symbol, API, schema, and behavioural overlap. Distinguish textual overlap from semantic dependency conflict.

Examples:

- One agent renames a type another agent imports.
- Two agents independently change retry semantics.
- One migration invalidates another agent's query.
- A generated client is stale relative to another branch's schema change.

### P-005: Team review bundle — P1/P2

Export a portable, signed session package containing approved content:

- Intent and constraints.
- Timeline and checkpoints.
- Final and selected intermediate diffs.
- Semantic claims with evidence/provenance.
- Validations and source hashes.
- Human decisions and unresolved items.
- Redaction manifest and omitted-content markers.

Support self-contained HTML and JSON first. Hosted collaboration can come later.

### P-006: Organisation policies and privacy — P2

- Allowed semantic providers/models.
- Local-only path classifications.
- Retention, redaction, and export rules.
- Agent adapter permissions.
- Required human gates.
- Signed project policy and recipe bundles.
- Audit log integrity and access control.

## Interface design

### TUI: glanceable live cockpit

The TUI should remain the fastest surface for developers already living beside an agent terminal.

Suggested layout:

```text
┌ Session / intent / mode / risk / checks ─────────────────────┐
├ Timeline & clusters ───────┬ Semantic or raw evidence ───────┤
│ ▸ Plan: token polling      │ Meaning: retry policy changed   │
│ ✓ Add state model          │ Evidence: poller.rs:44          │
│ ! Unexpected config edit   │ Impact: 3 callers, 2 tests      │
│ ● Tests running            │ [raw] [structure] [meaning]     │
├ Scope / checks / feedback ─┴─────────────────────────────────┤
│ 1 drift · 2 unreviewed · unit ✓ · integration stale          │
└───────────────────────────────────────────────────────────────┘
```

Key actions: next attention item, ask why, toggle truth layer, annotate, send feedback, checkpoint, compare time, pause/request attention, and open raw event.

### Desktop: spatial investigation and long-running supervision

The desktop workspace should specialise in:

- Multi-pane timeline, graph, diff, and validation views.
- Multiple agent/session tabs.
- Drag-and-drop intent documents and policies.
- Persistent background observation and actionable notifications.
- Visual blast-radius and multi-agent conflict maps.
- Rich session replay and report authoring.

Both interfaces consume the same session/event/semantic graph. They may differ in presentation, not truth.

### Headless: CI and agent protocol

Example commands:

```text
luminatti watch --session oauth-flow --intent task.md
luminatti run --policy luminatti.agent.toml -- codex
luminatti status --format json
luminatti ask "what changed outside scope?"
luminatti checkpoint create before-migration
luminatti review export --format html
luminatti policy check --session current --fail-on blocking
```

Headless output requires versioned JSON Schema and stable exit codes for clean, attention required, policy violation, validation failure, observation incomplete, control unavailable, and internal error.

## Semantic layer design

The semantic layer is central to the mission, but an external generative model does not need to be central to the first implementation.

### Layer 1: deterministic facts

- File/VCS metadata and raw diffs.
- AST and symbol changes.
- Imports, references, call relationships where resolvable.
- Schema/config/dependency changes.
- Test/build/static-analysis results.
- Intent and policy rule matches.

### Layer 2: deterministic and heuristic interpretation

- Rename/move/extract candidates.
- Generated-file relationships.
- Change episode correlation.
- Likely test-to-change mapping.
- Scope and risk rules.

### Layer 3: model-assisted semantics

- Behavioural summaries.
- Intent clusters across languages/files.
- Missing-work questions.
- Risk hypotheses.
- Natural-language queries.
- Suggested feedback and validation.

### Layer 4: human authority

- Confirmed intent.
- Accepted scope exceptions.
- Review decisions.
- Policy overrides.
- Approval to apply, restore, pause, or finish.

### Provider contract

Each semantic request declares:

- Purpose and user-visible question.
- Session and source-hash scope.
- Selected evidence and why it is needed.
- Redaction and forbidden-path policy.
- Maximum content/token/cost budget.
- Provider/model/analyser identity and version.
- Cache key, timeout, cancellation, and retention policy.
- Structured response schema requiring evidence references and confidence.

Users should be able to inspect the outbound payload plan before enabling a hosted provider. Local analysers and models use the same contract.

### Semantic safety rules

- Never hide a raw change solely because a model calls it unimportant.
- Never call a change behaviour-preserving without deterministic support or explicit uncertainty.
- Invalidate claims when their evidence hashes change.
- Separate agent self-explanation from Luminatti analysis.
- Preserve contradictory evidence.
- Never apply semantic suggestions directly to disk.
- Make “unknown” a first-class answer.
- Evaluate models on false reassurance, not only summary quality.

## Supporting comparison and merge features

These incumbent-style capabilities still matter, but only where they strengthen the agent mission.

### Required supporting features

- High-quality two-way and three-way code diff.
- Important, unimportant, and ignored differences with explainable profiles.
- Multiple diff algorithms and manual alignment.
- Encoding, BOM, line-ending, large-file, and binary correctness.
- Editable merge output with unlimited undo and atomic save.
- Git/Jujutsu conflict centre.
- Saved sessions, checkpoints, and portable reports.
- Arbitrary file, clipboard, stdin, VCS blob, and checkpoint comparison.
- Structured JSON/YAML/TOML/XML/CSV adapters.
- Deterministic CLI and library/API.

### Postpone unless demanded by agent workflows

- General folder mirroring and backup.
- FTP/SFTP/cloud file browsers.
- Registry and executable-resource comparison.
- Audio/media metadata comparison.
- Broad office-document extraction.
- Pixel image comparison beyond screenshot/snapshot-test workflows.
- Proprietary document editing.

### Agent-specific image comparison opportunity

Screenshot and visual-regression comparison is relevant when an agent changes UI:

- Baseline versus current screenshot.
- Pixel difference and perceptual difference.
- DOM/accessibility-tree change when available.
- Link visual change to CSS/component/source episode.
- Accept new baseline only through explicit review.

This should be prioritised ahead of generic photography comparison.

## Distinctive product features

### Agent heartbeat

A single-line continuously updated explanation of the agent's current meaningful activity, confidence, and whether human attention is needed. It replaces walls of command output, not the underlying terminal.

### Scope drift radar

Visualise distance from the intent contract over time. A small temporary deviation that is later reverted looks different from persistent unapproved scope expansion.

### Prompt-to-patch trace

Trace user request → plan step → agent action → repository change → test → human decision. Exact edges appear only with evidence; uncertain links are dotted and confidence-labelled.

### Semantic time machine

Follow a behaviour or symbol through the agent's attempts, including false starts and reversions, instead of comparing only baseline and final text.

### “What changed since I looked?”

Summarise only changes introduced after the user's last reviewed checkpoint, preserving already reviewed state and highlighting invalidated decisions.

### Review lenses

- Behaviour.
- Public API.
- Security and permissions.
- Data/schema.
- Dependencies/build/deploy.
- Tests and validation.
- Mechanical/generated noise.
- Outside scope.
- Since last review.

Every lens is an explainable query over deterministic and semantic graph data.

### Truth stack

Let users step through:

```text
Events → Bytes → Text → Structure → Meaning → Impact → Decision
```

This becomes Luminatti's signature trust model: users can always descend from an interpretation to its evidence.

### Change budget burn-down

Show how much of the expected change budget remains and which episode consumed it. This helps humans spot an agent turning a small task into a rewrite.

### Agent feedback receipt

After feedback, show what the agent acknowledged, what it changed, which exact request remains unmet, and what new side effects appeared.

### Counterfactual final review

Compare final work against the last passing checkpoint, original baseline, and stated acceptance criteria—not merely against HEAD.

### Uncertainty inbox

Collect all unresolved unknowns, weak causal links, stale semantic claims, unrun checks, agent assumptions, and human questions in one review queue.

## Delivery roadmap

### Release 0: sidecar foundation

- Agent session and protected baseline.
- Append-only event log and incremental snapshots.
- Observe and wrapped modes.
- Live timeline with raw diffs.
- Checkpoints and session persistence.
- Versioned event JSON and headless status.

**Proof:** Luminatti can watch a real agent session without losing or misattributing existing work, and produce an accurate time-ordered replay.

### Release 1: live review cockpit

- Intent contract and allowed/protected scope.
- Change episode correlation.
- Structural symbol diff.
- Scope drift and deterministic risk rules.
- Unified validation feed with freshness.
- Semantic annotations and structured feedback export.

**Proof:** a developer can supervise an agent without repeatedly reading the complete terminal stream or manually rebuilding context with `git diff`.

### Release 2: connected agents

- Versioned integration protocol and first agent adapters.
- Plan/tool/patch correlation.
- Agent heartbeat and “why did this change?”
- Feedback verification loop.
- Safe-point pause/attention requests where supported.
- Portable agent-session report.

**Proof:** Luminatti links agent intent and actions to independent repository evidence and measurably shortens review feedback cycles.

### Release 3: semantic intelligence

- Semantic change graph and intent clusters.
- Meaning cards and blast-radius map.
- Natural-language evidence queries.
- Missing-work questions and completion gate.
- Test-to-change mapping and differential validation plan.
- Local and hosted provider boundary with privacy controls.

**Proof:** reviewers understand large agent-generated changes faster without an increase in missed important changes or false reassurance.

### Release 4: control plane

- Project change contracts and budgets.
- Brokered patch API and review-before-write mode.
- Policy gates and source-hash preconditions.
- Safe partial restoration and counterfactual previews.
- Multi-agent lanes and conflict forecast.

**Proof:** teams can enforce selected human gates and safely recover agent-owned work without damaging unrelated changes.

## Measurement plan

### Observation correctness

- Pre-existing user change misattribution rate: target zero.
- Missed or duplicate filesystem/VCS event recovery rate.
- Accuracy of baseline, checkpoint, and final source hashes.
- Timeline ordering and session crash-recovery success.
- Agent ownership attribution precision by integration mode.

### Comprehension

- Time to answer “what changed and why?”
- Time from attention-worthy event to human awareness.
- Percentage of semantic claims opened to evidence.
- Reviewer agreement with change clusters and risk ranking.
- Structural move/refactor precision and recall.
- False “no meaningful change” and false “behaviour preserving” rates.

### Control and review

- Feedback cycles required before resolution.
- Feedback marked resolved but later found unmet.
- Scope drift detected before agent completion.
- Successful partial restore without loss of protected work.
- Percentage of sessions completed with all required checks fresh.
- Human overrides of policy/semantic recommendations and subsequent outcomes.

### Product value

- Reduction in time spent manually polling `git diff` and agent output.
- Percentage of active agent time supervised through Luminatti.
- Repeat use as default agent sidecar.
- Session report usage in pull requests or audits.
- Adoption of connected mode after observe-mode trust is established.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Event feed becomes noisier than agent terminal | Semantic episode grouping, quiet defaults, raw events on demand |
| Luminatti overstates causality | Evidence classes, integration-mode labels, dotted inferred edges |
| Semantic summaries create false trust | Raw truth stack, confidence, contradiction, staleness, false-reassurance evaluation |
| Observation slows agent or repository | Incremental parsing, bounded queues, backpressure, sampling of non-source artefacts |
| Rollback destroys user work | Protected baseline, attribution requirement, source-hash preconditions, previewed inverse patch |
| Agent integrations fragment | Small versioned protocol and adapter conformance suite |
| Users reject workflow interception | Observation-first adoption; connected and brokered modes remain opt-in |
| Sensitive code leaks to models | Local deterministic core, explicit provider boundary, path policies, payload inspection |
| Agents game policy through self-report | Independent filesystem/VCS/validation evidence remains authoritative |
| “Semantic” becomes vague marketing | Falsifiable meaning cards, evidence links, measurable evaluation corpus |

## Features to avoid

- A global “agent is safe” score.
- Invisible AI filtering of diffs.
- Automatic approval based only on model confidence.
- Claiming exact command causality in observe mode.
- Restoring the whole working tree when only agent-owned work should change.
- Recording secrets, full environment dumps, or unbounded terminal history by default.
- Running arbitrary validations without permission.
- Treating passing tests as proof of correctness.
- Letting an agent resolve or dismiss human review items unilaterally.
- Building hosted collaboration before the local session format is durable.
- Chasing generic comparison-suite breadth before live agent supervision works.

## Mission-complete checklist

Luminatti is fulfilling its core mission when:

- It can attach to or launch a coding agent and establish a protected baseline.
- It continuously shows meaningful work rather than only final text differences.
- Every change is attributable at the highest evidence level available, without invented certainty.
- The human can see intent, structure, semantic meaning, impact, validation, and uncertainty together.
- Scope drift and important risk surface while the agent can still respond.
- Review feedback returns to the agent with stable evidence anchors.
- Luminatti verifies whether subsequent work addressed that feedback.
- Checkpoints and restoration never destroy pre-existing user work.
- The complete session can be replayed and exported.
- The deterministic observer works locally with no model configured.
- Model-assisted semantics always link to evidence and can say “unknown.”
- Connected and brokered modes add control without making the read-only mode less useful.

## Sources and benchmark references

The competitor research remains relevant for the deterministic comparison foundation, but not as the product's mission.

### Araxis Merge

- [Product overview](https://www.araxis.com/merge/)
- [Text comparison and editing](https://www.araxis.com/merge/windows/comparing-text-files.en)
- [Three-way automatic file merging](https://www.araxis.com/merge/windows/automatic-file-merging.en)
- [Three-way folder comparison](https://www.araxis.com/merge/windows/three-way-folder-comparison.en)
- [Saved comparison archives, bookmarks, and comments](https://www.araxis.com/merge/windows/saving-comparisons-for-archival-or-team-collaboration.en)
- [Command-line interface](https://www.araxis.com/merge/windows/command-line.en)

### Beyond Compare

- [Product overview](https://www.scootersoftware.com/)
- [Standard versus Pro](https://www.scootersoftware.com/v5help/standard_vs_pro.html)
- [Version 5 features](https://www.scootersoftware.com/download/v5whatsnew)
- [Command-line reference](https://www.scootersoftware.com/v5help/command_line_reference.html)
- [Automation scripting](https://www.scootersoftware.com/v5help/scripts.html)

### Luminatti repository evidence

- `README.md`
- `src/command/diff/`
- `src/vcs/`
- `src/desktop/`
- `src/provider/`

