---
description: Evaluation-first phases for corpus processing, ranked retrieval, the Rust TUI, feedback, and later context search.
title: Roadmap
order: 70
---

::page{layout="docs" width="normal" sidebar=true}

# Roadmap

MamboMeme is currently documentation only. The build order proves a searchable corpus and ranked results before adding source breadth, sophisticated models, or context-aware suggestions.

Every phase adds its own tests and exits on evidence. A partially working phase does not justify scaffolding later phases.

## Phase 0: Freeze contracts and fixture

Deliver:

- source envelope, SQL migrations, corpus manifest, worker protocol, query, result, and optional interaction-event schemas;
- a small cleared fixture containing text and image items, exact duplicates, one quarantine case, and a development-only expected John Cena result group;
- development benchmark, relevance guide, safety policy, query slices, and empty scorecard;
- hand-calculated evaluator fixtures;
- documented runtime and hardware profile.

Exit when a second build plan can reproduce the same canonical content identity from the documented inputs and every contract has an owning language.

## Phase 1: Data vertical slice

Deliver:

- one Rust `ingest` command for the local fixture manifest;
- bounded text and image parsing, rights validation, hashes, provenance, outcomes, deduplication, cursor transactions, and quarantine;
- shared SQL migrations and content-addressed media storage;
- one Python build command for reviewed metadata, OCR fixture handling, FTS, embeddings, and manifest creation;
- validation and atomic publication with rollback tests;
- pipeline health and resource report.

Exit when two fixture imports are idempotent, invalid items cannot enter the serving index, a failed candidate build leaves the active snapshot intact, and every Phase 1 test passes.

Do not add an external crawler or generic source-adapter hierarchy yet.

## Phase 2: Ranked retrieval

Deliver in this order:

1. a Python BM25 baseline that retrieves the development-only `john cena` fixture, template names, and quote fragments;
2. a dense text baseline for semantic and visual-description queries;
3. deterministic hybrid fusion, filters, duplicate collapse, and empty-result behavior;
4. the long-lived NDJSON worker with version handshake and typed failures;
5. benchmark rows, ablations, latency/memory measurements, and MMTS-Search-v1;
6. an error analysis identifying missed fields, weak labels, and false nearest neighbours.

Exit when the frozen comparison decides whether semantic search meets its acceptance rule. Ship BM25 alone if it does not.

## Phase 3: Rust TUI and selection

Deliver:

- a Ratatui interface for query input, ranked results, preview, navigation, open/copy/select, empty, and error states;
- one worker per TUI session with bounded startup, query, restart, and shutdown behavior;
- portable text/metadata preview plus a measured inline-image adapter if it is reliable;
- deterministic state tests and PTY terminal-restoration smoke tests;
- opt-in local interaction-event capture, inspection, disable, retention, and deletion;
- end-to-end submit-to-render latency and the complete fixture selection path.

Exit when a user can launch the application, search `john cena`, inspect the ranked items, select the expected stable ID, and quit without terminal corruption. The first useful portfolio release ends here.

Mouse support, themes, a graphical application, and uploaded telemetry are not required.

## Phase 4: Repeatable external ingestion

Deliver:

- one approved external source adapter;
- allowlisted and pinned-address requests, redirect validation, rate limiting, bounded retries, and resumable cursors;
- source-specific rights, retention, attribution, edit, and deletion behavior;
- OCR and annotation processing within resource limits;
- near-duplicate/template review tooling only where fixture evidence requires it;
- corpus freshness, deletion lag, throughput, and resource measurements.

Exit when interruption and rerun cannot duplicate or skip serving records, one bad item cannot lose the batch, and a permission change removes an item predictably.

Extract shared adapter code only after a second real source reveals duplication.

## Phase 5: Retrieval learning experiments

Run only the experiments supported by measured errors.

Possible deliverables:

- CLIP-style image embeddings for the visual-description slice;
- reviewed hard negatives from search failures and opt-in selections;
- an offline reranker or fine-tuned encoder;
- template-grouped training, development, and fresh hidden-test partitions;
- model card, reproducible training configuration, before/after scorecard, and ablations.

Exit only when a frozen model improves a fresh hidden benchmark with a positive paired confidence bound and no exact-search, safety, latency, or reliability regression. Remove experiments that do not help.

Raw selection counts never become labels automatically. Review them or use a controlled exposure design first.

## Future: Context-aware suggestions

This is the fourth product component but not a first-release dependency. It accepts either one sentence or a bounded sequence of role-labelled messages and returns search suggestions or ranked meme candidates through the existing retrieval contract.

Compare the smallest approaches in order:

1. search only the latest message;
2. search a bounded concatenation of recent role-labelled messages;
3. use one model call to derive several short search cues, then run normal retrieval.

Keep the cheapest approach that wins a separate blind usefulness evaluation. Context remains ephemeral by default, never enters selection telemetry, and requires its own privacy, prompt-injection, safety, latency, and conversation-boundary tests.

Do not create context modules, prompts, storage, or benchmark files before this phase begins.

## Decisions made now

| Decision | Reason |
|---|---|
| Search and rank existing items | Matches the actual user workflow and creates a testable retrieval task. |
| User chooses the result | Ranking assists discovery; it does not generate or send a reply. |
| Rust owns ingestion and TUI | Uses Rust for source boundaries, terminal lifecycle, and systems practice. |
| Python owns ML and retrieval | Keeps models, embeddings, evaluation, and one ranking implementation together. |
| NDJSON subprocess boundary | Loads Python once without adding HTTP, FFI, or duplicate services. |
| BM25 before semantic search | Exact entity, name, and quote search is the mandatory baseline. |
| Hybrid only if measured | Dense retrieval should earn its latency and complexity. |
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

## First-release proof

The portfolio story is complete when it shows:

1. safe Rust processing of a fixed source fixture;
2. a reproducible, rights-aware searchable corpus;
3. BM25, dense, and hybrid benchmark results with a justified winner;
4. a Rust TUI that lists, previews, and selects ranked results;
5. passing contract, security, integration, evaluator, TUI, and end-to-end tests;
6. performance measurements and MMTS-Search-v1 with its components and gates;
7. an error analysis and honest next experiment.

Multiple crawlers, a vector database, an HTTP API, a polished GUI, trained reranking, live learning, and context-aware replies are not required for that proof.
