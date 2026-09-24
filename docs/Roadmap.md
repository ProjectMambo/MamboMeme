---
description: Four substantial implementation phases for the local corpus, ranked retrieval, Rust TUI release, and one approved external source.
title: Roadmap
order: 70
---

::page{layout="docs" width="normal" sidebar=true}

# Roadmap

MamboMeme is built in four substantial phases. Each phase leaves a useful, executable system and closes only after its acceptance checks pass. Context-aware suggestions remain future work outside these four phases.

Phase 1 is complete. Its Rust ingester, Python index builder, migration, cleared fixture, and acceptance checks are implemented. Phase 2 ranked retrieval is next; the TUI and external acquisition are not implemented yet.

## Phase 1: Executable local corpus

**Status: complete.** Closed with deterministic local ingestion and publication, ten Rust tests, ten Python tests, Clippy with warnings denied, and the offline cross-language fixture.

This phase combines the former contract and data-vertical-slice phases. It establishes the durable boundary that every later feature consumes.

Deliver:

- one Cargo package and one Python package, with no service or plugin framework;
- versioned source-envelope, SQL, snapshot-manifest, normalization, and outcome contracts;
- a small redistributable fixture containing text and image items, exact duplicate content, and a rights-invalid quarantine case;
- one Rust `ingest` command for bounded local JSON Lines records and static images;
- local-path containment, bounded static PPM decoding and dimension checks, rights validation, content hashing, exact deduplication, provenance, explicit outcomes, and content-addressed media storage;
- ordered SQLite migrations, idempotent whole-manifest transactions that roll back every canonical change, and an unresolved-manifest gate that fails closed after interruption or fatal failure;
- one Python `build_index` command that builds deterministic fielded FTS5, validates integrity and serving coverage, checksums the snapshot, and atomically publishes `active.json`;
- Rust and Python tests for the fixture, invalid inputs, reruns, deterministic output, and publication rollback.

Phase 1 deliberately excludes OCR/model calls, embeddings, search APIs, the worker protocol, the TUI, and network acquisition.

Exit when:

1. the documented commands run from a clean checkout;
2. two imports preserve canonical IDs and item counts while producing the documented accepted/duplicate then unchanged outcomes;
3. the duplicate retains provenance without creating another searchable item;
4. the invalid-rights record remains quarantined and outside FTS;
5. two builds produce the same canonical content identity and search rows;
6. a failed candidate build leaves the active snapshot unchanged;
7. all Phase 1 Rust and Python tests pass offline.

## Phase 2: Ranked retrieval and evaluation

Build the complete headless search engine on the published Phase 1 corpus.

Deliver in this order:

1. a Python BM25 baseline over the fielded FTS index for people, template names, tags, and quote fragments;
2. query validation, eligibility filters, duplicate/template handling, deterministic ties, and correct empty results;
3. the frozen development and hidden benchmarks, evaluator tests, latency and memory measurements, error analysis, and MMTS-Search-v1 report;
4. a dense text baseline for semantic and visual-description queries;
5. deterministic hybrid fusion and ablations against the same benchmark;
6. the long-lived, versioned NDJSON worker with a handshake, one in-flight query, typed errors, timeouts, and clean shutdown.

Exit when the worker returns reproducible ranked results and the frozen BM25-versus-hybrid comparison decides whether semantic search earns its complexity. Ship BM25 alone when the dense route fails its documented acceptance rule.

Image embeddings, approximate-nearest-neighbour indexes, a vector database, and a learned reranker are not Phase 2 defaults. Add an experiment only when error analysis identifies a specific gap.

## Phase 3: Rust TUI and first release

Complete the user workflow without changing retrieval ownership.

Deliver:

- a Ratatui interface for query input, ranked results, preview, navigation, open/copy/select, empty results, and typed errors;
- one Python worker per TUI session with bounded startup, query, restart, and shutdown behavior;
- portable text and metadata previews, with inline images only if one maintained adapter proves reliable;
- deterministic state and rendering tests plus PTY terminal-restoration smoke tests;
- opt-in local interaction-event capture, inspection, immediate disable, retention, and deletion;
- end-to-end submit-to-render measurements and the complete fixture selection path;
- a reproducible release scorecard, usage documentation, and packaged first release.

Exit when a user can launch MamboMeme, search the fixture, inspect ranked items, explicitly select the expected stable ID, and quit through success and failure paths without terminal corruption. The release must pass the relevant MMTS-Search-v1 gates.

Mouse support, themes, a GUI, uploaded telemetry, and context-aware replies do not block the release.

## Phase 4: One approved external source

Replace the local-only acquisition boundary with one real, policy-reviewed source while preserving the same canonical corpus contract.

Deliver:

- one approved external source integration, without a generic adapter hierarchy;
- documented API/automation permission, retention, redistribution, attribution, indexing, deletion, and takedown rules;
- allowlisted and pinned-address requests, redirect validation, response limits, rate limiting, bounded retries, and resumable cursors;
- source-specific update and deletion handling;
- resource-limited OCR or annotation only where the source needs it;
- corpus freshness, deletion lag, throughput, failure, and resource reports;
- offline network-boundary tests using a controlled local server and resolver stub.

Exit when interruption and rerun cannot duplicate or skip serving records, one bad item cannot lose the batch, a rights or permission change removes an item predictably, and the enlarged corpus still passes retrieval, safety, latency, and integrity gates.

Extract shared source-adapter code only after a second approved source exposes real duplication.

## Closing a phase

Every phase closes with the same small release discipline:

1. run that phase's acceptance commands and record the result;
2. update the canonical MamboMeme documents with implemented behavior, measured results, and remaining boundaries;
3. sync the canonical documents into the MamboMeme repository;
4. review the generated documentation diff alongside the implementation diff;
5. commit one coherent phase result and push `main`.

Do not mark a phase complete, sync aspirational behavior as current, or begin scaffolding the next phase before the current acceptance checks pass.

## Future: Context-aware suggestions

This remains a separate product iteration. It accepts either one sentence or a bounded sequence of role-labelled messages and derives search cues for the existing retrieval contract.

Compare the smallest approaches in order:

1. search only the latest message;
2. search a bounded concatenation of recent role-labelled messages;
3. use one model call to derive several short search cues, then run normal retrieval.

Keep the cheapest approach that wins a separate blind usefulness evaluation. Context stays ephemeral by default, never enters selection telemetry, and requires its own privacy, prompt-injection, safety, latency, and conversation-boundary tests.

Do not create context modules, prompts, storage, or benchmark files during the four implementation phases.

## Decisions made now

| Decision | Reason |
|---|---|
| Search and rank existing items | Matches the actual user workflow and creates a testable retrieval task. |
| User chooses the result | Ranking assists discovery; it does not generate or send a reply. |
| Rust owns ingestion and the TUI | Uses Rust for source boundaries, terminal lifecycle, and systems practice. |
| Python owns indexing, retrieval, and evaluation | Keeps FTS, models, embeddings, evaluation, and one ranking implementation together. |
| NDJSON subprocess boundary | Loads Python once without adding HTTP, FFI, or duplicate services. |
| BM25 before semantic search | Exact entity, name, and quote search is the mandatory baseline. |
| Hybrid only if measured | Dense retrieval must earn its latency and complexity. |
| Exact NumPy scan first | It is sufficient at portfolio scale until measured otherwise. |
| Selection feedback is offline evidence | Raw behavior is biased and cannot safely self-train a ranker. |
| Context is future work | It is a different task built on top of working search. |

## Upgrade triggers

| Add | Trigger |
|---|---|
| Tokio or async ingestion | Measured permitted concurrency improves real source throughput. |
| Approximate vector index | Exact dense search fails the real latency gate. |
| PostgreSQL or a vector service | Concurrent remote users and shared writes require them. |
| PyO3 | A profiled fine-grained boundary dominates after batching. |
| HTTP service | Rust and Python must deploy or scale independently. |
| GUI | Terminal preview, clipboard, or accessibility needs limit real users. |
| Background queue | One resumable process cannot handle the real corpus workflow. |
