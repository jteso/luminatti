# Luminatti semantic-ingestion subsystem

## Summary

Add a local-first, read-only subsystem that turns harness activity and repository changes into a durable semantic change graph and evidence-backed impact and risk assessments. It combines independent filesystem/VCS evidence with Codex turn-complete notifications, Tree-sitter structural deltas, and opt-in LSP enrichment.

The first user experience is a desktop timeline that explains what changed, which consumers are affected, the compatibility impact, available test evidence, and recommended follow-up actions. Users can drill into each finding's files, hunks, symbols, references, provenance, and analysis confidence. SQLite is the embedded graph store; no external graph database, daemon API, MCP server, or harness control is included.

## Processing pipeline

**Change observation -> Event ingestion -> Structural analysis -> Relationship enrichment -> Semantic graph -> Impact assessment -> Risk assessment -> Explanation and recommendations**

1. **Change observation:** Capture the protected baseline, filesystem/VCS changes, source snapshots, and Codex turn boundaries. Distinguish pre-existing changes from changes observed during the session.
2. **Event ingestion:** Normalize, deduplicate, and persist events with source, timestamps, session/episode association, and evidence classification.
3. **Structural analysis:** Compare old/new snapshots with Tree-sitter to identify symbol additions, removals, signature changes, implementation changes, and imports. Preserve raw diffs when structural analysis is unavailable.
4. **Relationship enrichment:** Use optional LSP definitions and references to identify callers and consumers. Map references to distinct modules/services using repository configuration or explicit boundary mappings; retain unresolved references without inventing service identities.
5. **Semantic graph:** Connect changes, symbols, consumers, modules/services, tests, and validation evidence with provenance. The graph is the shared evidence store updated throughout processing, rather than a one-time artifact created only after enrichment.
6. **Impact assessment:** Determine affected consumers, potential or confirmed compatibility breaks, and relevant tests. Distinguish direct references from inferred transitive impact, and report the scope and completeness of dependency discovery.
7. **Risk assessment:** Apply versioned, deterministic rules to compatibility impact, affected consumers, and available validation evidence. Report low, medium, high, or unknown risk with reasons; keep risk separate from evidence provenance and analysis confidence.
8. **Explanation and recommendations:** Produce a per-episode summary in the order: what changed -> affected consumers -> compatibility impact -> test evidence -> risk and recommendation. Link every finding to supporting evidence and expose limitations.

## Assessment behavior

- A signature change is a structural fact, not automatically a backwards-incompatible change. Use supported language-specific compatibility rules and resolved consumer evidence; label unsupported or ambiguous cases as potential breaks.
- Count distinct affected services only when their boundaries are known. Otherwise report resolved callers, symbols, or files and state that service-level impact is unavailable. Include the dependency path for inferred transitive impact.
- Treat additions within established modules as context, not sufficient evidence of low risk. Low risk requires supported analysis showing an additive/compatible change, no detected disruption to existing consumers within the analyzed scope, and relevant passing validation for the assessed snapshot.
- Assign high risk when a confirmed backwards-incompatible change affects existing consumers or a relevant validation fails. Assign medium risk when analysis identifies a potential break or an understood impact that needs validation. Use unknown when evidence is insufficient to assess impact; incomplete discovery must never imply low risk. Known high-risk findings remain high even when other impact is unknown.
- Report test changes, execution results, and coverage as separate evidence. “No relevant test updates detected” does not mean existing coverage is inadequate; “tests passed” does not establish coverage of affected behavior. Claim coverage only when supplied coverage data maps to that behavior.
- Associate validation with the assessed source hashes/revision and record its scope and origin. Ingest externally supplied test/coverage results; do not automatically execute repository tests. Missing test mappings, results, or coverage remain explicitly unknown, and declared results retain their declared provenance.
- Persist findings with the assessed snapshot, rule version, affected entities, compatibility status, risk, evidence links, limitations, and recommended actions. Mark assessments stale after relevant source changes and recompute for the new snapshot; retain previous findings for timeline inspection.
- Generate recommendations from observed gaps: update incompatible callers, run relevant tests when current results are absent, or extend regression coverage when behavior is unverified. Recommendations do not modify source or send feedback to the harness.

### Example summaries

> **Low risk:** Added services within existing modules. No existing public signatures changed, and no disruption to existing callers was detected within the analyzed scope. Relevant tests passed for this snapshot.

> **High risk:** Changed an interface signature incompatibly, affecting five resolved services. No updates to the relevant tests were detected, and validation of the affected behavior is unavailable. Update affected callers and extend regression coverage to verify existing behavior.

> **Impact unknown:** Changed a public signature, but reference enrichment was unavailable. Consumer impact and backwards compatibility could not be established. Restore reference analysis and validate callers before relying on this change.

Each summary includes evidence links and analysis limitations. The service count, compatibility conclusion, and validation statements must be supported by the recorded evidence.

## Key changes

- Add a core `semantic` module, available to both CLI and desktop builds. If persistence is needed, keep it in application-managed data rather than repository metadata, using WAL and bounded changed-file snapshots.
- Define a versioned JSONL ingestion contract: event id, session id, timestamp, source, event kind, optional parent/action id, payload, and evidence class. Support session, intent, plan, step, tool, patch, validation, message, attention, and completion events.
- Add read-only commands:
  - `luminatti session start` creates a protected baseline and active session.
  - `luminatti session ingest --session <id>` consumes generic JSONL from stdin.
  - `luminatti session codex-notify <json>` records a Codex turn-complete boundary for the active session matching the notification's working directory.
  - `luminatti session status|show --json` exposes timeline, episode, symbol, evidence, and assessment queries for scripts, including risk reasons, recommendations, analysis scope, unknowns, and stale status.
  - `luminatti session open` launches the desktop cockpit for the active or selected session.
- Build a Codex adapter around its documented `notify` callback. Users manually configure the reversible notification command; it supplies turn boundaries only. Luminatti must label source edits in a turn as **correlated**, never claim exact tool- or patch-level causality.
- Add an observer that records the initial Git/VCS state, debounced filesystem changes, source hashes, incremental diffs, renames/deletions, and reconciliation scans. Pre-existing dirty files remain protected context and are never attributed to Codex.
- Project immutable events into SQLite node/edge tables for sessions, turns, files, snapshots, hunks, symbols, structural deltas, modules/services, tests, validations, claims, and assessments. Store provenance on every edge; use indexed SQL and recursive queries as the graph engine.
- Run existing Tree-sitter grammars over only changed old/new snapshots. Produce deterministic symbol additions, removals, moves/renames where reliable, signature changes, imports, and changed-span anchors.
- Add opt-in LSP enrichment via local stdio servers. Detect configured/installed servers per language, request document symbols, definitions, and references only for changed symbols, and record unavailable/failed enrichment as evidence rather than silently falling back.
- Add local impact and risk evaluators over the semantic graph, with supported compatibility rules, consumer boundary mapping, test/validation association, versioned risk rules, and evidence-backed recommendation templates. Unsupported analysis produces explicit unknowns.
- Extend the native desktop reviewer with a session mode: live timeline -> selected episode -> impact/risk summary and recommendations -> affected consumers/tests -> supporting files/hunks, structural deltas, and references. Every item displays direct, declared, correlated, or derived evidence classification separately from risk and analysis confidence, and links back to supporting evidence.

## Test plan

- Unit-test JSONL versioning, idempotent event ingestion, session routing, evidence classification, graph projection, and query ordering.
- Use temporary Git repositories to verify protected baselines, bursty file writes, rename/delete handling, reconciliation after missed events, and snapshot retention.
- Test Tree-sitter structural deltas across Rust, TypeScript, Python, and unsupported/invalid syntax fallback.
- Use a fake JSON-RPC language server to verify opt-in LSP requests, timeout/error recording, and no server launch when disabled.
- Add desktop model tests for timeline grouping, correlated-versus-direct labels, selected-symbol evidence, and stale analysis after a subsequent file change.
- Verify distinct service counting, unresolved service boundaries, direct versus transitive impact paths, and potential versus confirmed compatibility breaks.
- Test assessment rules for additive changes with passing validation, incompatible signatures affecting existing consumers, relevant validation failures, and missing/partial LSP evidence. Verify incomplete analysis never produces low risk and cannot downgrade a confirmed high-risk finding.
- Verify unchanged tests are not classified as missing coverage, absent results are not classified as failures, passing tests are not treated as coverage proof, and validation from an earlier snapshot cannot validate the current change.
- Test assessment persistence, deterministic rule results, evidence links, stale/recomputed findings, and equivalent desktop/JSON output. Use fixtures for the example summaries, including five distinct affected services.
- Exercise the Codex notify adapter with documented `agent-turn-complete` payloads; verify it never records a more precise causal claim than the event supports.

## Assumptions

- V1 is observation and explanation only: it cannot pause Codex, send feedback, create checkpoints, or modify source.
- V1 assessments use local deterministic rules and templates. They do not require a hosted model, automatically run tests, or guarantee complete dependency discovery or behavioral compatibility.
- Codex integration uses only turn-complete notifications, not OpenTelemetry or transcript scraping; generic JSONL remains the path for richer future adapters.
- LSP enrichment is disabled unless explicitly configured and remains local; no source or telemetry is sent to a network service.
- Session evidence stays local in application-managed data as hashes, metadata, and only baseline/changed snapshots needed for replay. Luminatti will not add repository metadata or edit `.gitignore` automatically.
