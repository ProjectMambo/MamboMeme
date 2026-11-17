---
description: Components, Rust and Python ownership, process boundaries, data flow, contracts, and repository shape.
title: Architecture
order: 10
---

::page{layout="docs" width="normal" sidebar=true}

# Architecture

## Product boundary

MamboMeme searches a local corpus of existing meme images and quotes. A user enters a query, inspects a ranked list, and selects an item. The first release does not invent a reply, generate an image, read a conversation, or autonomously act on the user's behalf.

The primary question is:

> Given this search query, which stored meme assets are most useful to show first?

For `john cena`, the engine should surface assets whose title, people, template, tags, OCR, or other metadata identify John Cena. Semantic evidence supplements this direct evidence; it does not redefine the query as a conversation.

## Four components

```text
1. DATA PROCESSING AND STORAGE
   source -> validated canonical item -> searchable corpus snapshot

2. RETRIEVAL
   query -> lexical and semantic candidates -> ranked results

3. USER INTERFACE
   search -> list -> preview -> selection

4. CONTEXT ASSISTANT — FUTURE
   one message or short chat -> search cues -> normal retrieval
```

The first three components form the first useful release. The fourth reuses them later; it is not a dependency of the search product.

Delivery uses four implementation stages rather than one stage per product component: Stage 1 creates an executable local corpus, Stage 2 creates ranked retrieval and evaluation, Stage 3 creates the TUI source release, and the paused Stage 4 would add one approved external source. See the [Development plan](Development%20Plan.md).

## Language ownership

The full system keeps one owner for each responsibility:

| Rust application | Python search package |
|---|---|
| Local intake, first-pass trust-boundary validation, hashing, provenance, exact deduplication, staging, terminal lifecycle, TUI state, result presentation, and optional local interaction-event capture | Search-document construction, FTS and dense retrieval, rank fusion, filters, evaluation, and the long-lived search worker |

Rust is useful for a distributable terminal program, careful handling of source bytes, and systems practice. Python keeps the ML ecosystem close to the model and evaluation code. This split is not a blanket claim that Rust makes every operation faster:

- SQLite FTS executes inside SQLite whichever language submits the query.
- NumPy and model frameworks already perform heavy numeric work in native kernels.
- Model encoding is likely to dominate search latency on a small local corpus.

Keep one implementation of each responsibility. Measure before moving a boundary.

## Current implementation boundary

Stages 1 through 3 are complete and the project is paused. Stage 1 established the durable corpus boundary:

```text
cleared local JSONL manifest and static fixture assets
    -> Rust sets an unresolved-manifest gate, validates, hashes, deduplicates, and commits one manifest transaction
    -> successful commit clears the gate; failure blocks other manifests and publication
    -> Python builds deterministic fielded FTS5 in a candidate snapshot
    -> Python validates integrity and coverage, checksums artifacts, and atomically publishes active.json
```

Phase 2 extends that published snapshot without changing ownership:

```text
bounded cues + kind/language filters
    -> weighted FTS5/BM25 lexical route
    -> optional TF-IDF/LSA exact-cosine route
    -> deterministic reciprocal-rank fusion and stable ties
    -> ranked items through a strict UTF-8 NDJSON worker
    -> provisional fixture evaluator and performance report
```

Phase 3 completes the local interaction path:

```text
explicit query submission in a Ratatui/Crossterm TUI
    -> one background Rust controller talks to one Python worker per session
    -> validated results render as a ranked list plus portable metadata preview
    -> open, manual-copy, or explicit stable-ID selection
    -> optional private JSONL interaction event
    -> bounded shutdown and terminal restoration
```

The paused Stage 4 design would extend only the acquisition side:

```text
finite human-reviewed Wikimedia Commons page-ID plan
    -> fixed Action API metadata request and verified original-media download
    -> pinned content, identity, rights, safety, and display-metadata comparison
    -> resumable raw/media/source-envelope staging
    -> the existing Rust ingest and Python publication boundary
    -> explicit tombstone or changed-rights replay invalidates active.json
```

The initial migration, cleared fixture data, Rust commands, Python builder, retrieval routes, evaluator, protocol types, worker, TUI, feedback log, profiler, unit tests, and Phase 1–3 offline cross-language fixtures are implemented. The Commons client, JPEG/PNG intake, deletion records, stale-worker protection, controlled network tests, and source report are not released.

There are no network requests, OCR calls, pretrained model downloads, image embeddings, inline terminal images, platform clipboard writes, uploaded telemetry, or context processing. The only planned network boundary is the allowlisted Commons metadata and original-media acquisition described in [Wikimedia Commons source](Wikimedia%20Commons%20Source.md).

## Full corpus build

```text
cleared local manifest
    -> Rust reads, validates, hashes, deduplicates, and stages
    -> raw content-addressed assets + SQLite provenance rows
    -> Python performs constrained OCR and enrichment only when a source needs them
    -> Python builds fielded FTS documents and declared dense representations
    -> Python validates and atomically publishes a corpus snapshot
```

The stages do not write concurrently. Rust completes its local-ingest transaction before Python index construction begins. The interactive search worker opens only the published snapshot and treats it as read-only.

Phase 2 adds a model-free TF-IDF/LSA representation and retrieval. Pretrained text/image models remain later measured experiments; a resumed Stage 4 must add source acquisition without changing retrieval ownership.

## Interactive search — Phases 2 and 3

```text
user submits query in Rust TUI
    -> TUI sends versioned NDJSON search request
    -> long-lived Python worker validates and normalizes the query
    -> BM25 lexical candidates + dense semantic candidates
    -> deterministic fusion, eligibility filters, and duplicate collapse
    -> NDJSON ranked-result response
    -> TUI renders list and selected-item preview
    -> user opens, copies, or selects one item
```

Phase 2 creates the worker and headless retrieval. Phase 3 starts one worker per TUI session so the index and corpus load once. Search runs on a controller thread while the terminal event loop remains responsive. There is no HTTP server, embedded Python, per-query process, or duplicate Rust search implementation in the source release.

## Online protocol — implemented in Phase 2

Use UTF-8 newline-delimited JSON over the worker's standard input and standard output. Standard error is diagnostic output only. Every message has `protocol_version`, `type`, and `request_id` where applicable.

The worker receives the corpus path as a launch argument, validates the complete snapshot, and emits `ready`. No extra `hello` message repeats command-line configuration. The minimal sequence is:

```text
Rust                              Python
 |       starts worker ------------>|
 |<----- ready ---------------------|
 |------ search(request_id) -------->|
 |<----- results(request_id) -------|
 |------ shutdown ----------------->|
 |<----- bye -----------------------|
```

Required message types:

| Type | Direction | Purpose |
|---|---|---|
| `ready` | Python to Rust | Confirm compatible protocol, loaded corpus, and retriever versions. |
| `search` | Rust to Python | Submit bounded query cues, filters, and result limit. |
| `results` | Python to Rust | Return ranked items and route evidence. |
| `error` | Python to Rust | Return a typed request-scoped or fatal failure. |
| `shutdown` | Rust to Python | Request deliberate session closure. |
| `bye` | Python to Rust | Confirm deliberate closure. |

The implemented worker bounds input lines at 64 KiB and output lines at 16 MiB, writes UTF-8 bytes independent of the process locale, accepts one request at a time, rejects duplicate request IDs, and distinguishes recoverable request errors from fatal framing, protocol, artifact, and search errors. Oversized input fails immediately without waiting for a newline. A result list that would exceed the output bound drops tail results and sets `truncated`; one individually oversized result is fatal. Invalid JSON is recoverable; an oversized or partial line, protocol mismatch, premature input EOF, or internal search failure emits a fatal error and exits non-zero. A valid `shutdown` produces `bye` and exit zero.

The Phase 3 Rust client validates the ready handshake, request ID, corpus/retriever versions, requested and announced routes, result count, rank sequence, safe flag, and non-empty unique result IDs. Each protocol response has a five-second timeout. The UI permits one in-flight submission, treats request validation errors as recoverable, treats worker/protocol failures as fatal, and offers at most one deliberate `r` restart per session. A stale or mismatched response is rejected before it can replace the displayed result set.

The paused Stage 4 must also close the stale-process path. Its planned worker check rereads the bounded active pointer before each search so a deletion, rights change, or replacement snapshot cannot leave an already-open SQLite handle serving removed content. This is an acceptance requirement, not current Phase 3 behavior.

## Artifact contract

The table separates the released contract from planned artifacts. Phases 1 and 2 implement local media, SQLite, migrations, the published manifest, FTS, and LSA artifacts; Phase 3 implements optional interaction events. Stage 4 acquisition artifacts remain planned and must not become a second canonical database.

Rust and Python exchange durable, inspectable build artifacts:

| Artifact | Owner | Consumer |
|---|---|---|
| Reviewed source plan, retained bounded API payload, cursor, and acquisition report — planned Stage 4 | Future Rust acquisition command writes | Human review and Rust ingestion consume |
| Content-addressed raw media | Rust writes | Python reads within the enrichment sandbox |
| Source, rights, outcome, and processing rows in SQLite | Rust writes | Python enriches during a stopped build |
| Ordered, language-neutral SQL migrations | Shared contract | Both apply or inspect |
| Field-labelled search documents and FTS tables | Python writes | Python worker reads |
| `dense_ids.json`, vocabulary/IDF files, SVD components, and L2-normalized dense vectors | Python writes | Python worker reads |
| Published manifest and checksums | Python writes | Python worker verifies at startup |
| Optional interaction events in versioned JSON Lines | Rust writes | Offline Python analysis reads |

The manifest records schema, builder, normalization, search-document, representation, NumPy and SQLite versions; canonical content identity; dense method and dimension; item/vocabulary counts; and every artifact checksum. Corpus identity is computed from a canonical ordered export rather than SQLite page bytes or timestamps. The snapshot identity also protects the derived artifact manifest.

The Phase 1 portion of one fixed fixture proves ingestion, outcomes, publication, and reproducibility. Phases 2 and 3 extend the same fixture through retrieval and selection. A resumed Stage 4 must add a controlled-server acquisition and deletion replay after the released sequence:

```text
Rust fixture import
    -> expected accepted, duplicate, and quarantine outcomes
    -> Python FTS index build and atomic publication
    -> Phase 2 worker returns the development-only "john cena" fixture at rank 1
    -> both languages decode the same golden protocol messages
    -> a second build preserves content identity and ranking
    -> Phase 3 Rust TUI receives and selects the intended stable ID

planned Stage 4:
    -> reviewed source record enters the same index
    -> explicit deletion invalidates publication
    -> rebuilt corpus excludes the item and the stale worker stops
```

## Search request

| Field | Contract |
|---|---|
| `request_id` | Session-unique identifier used to pair responses. |
| `cues` | One to four non-empty search cues, each at most 512 UTF-8 bytes. Multiple cues are alternate OR-style hints, not conversation turns. |
| `limit` | Positive result count, default `10`, maximum `50`. |
| `filters.kind` | Optional `text` or `image` filter. |
| `filters.language` | Optional simple language tag; omission means no language filter. |
| `route` | `lexical` by default; `dense` and `hybrid` are explicit experiment routes. |

## Search result

| Field | Contract |
|---|---|
| `id`, `kind` | Stable corpus identity and item type. |
| `title`, `text`, `asset_uri` | Display fields; text and image items have different required fields. |
| `caption` | Portable preview material when available. |
| `people`, `template`, `tags` | Searchable identity and grouping metadata. |
| `source`, `attribution` | Provenance required by the source policy. |
| `matched_fields`, `routes` | Explain whether names, tags, OCR, lexical, or dense evidence contributed. |
| `rank` | Final one-based position; internal scores are not probabilities. |
| `scores` | Diagnostic lexical/dense ranks and fused evidence, never a probability. |
| `truncated` | Worker-envelope flag showing that tail results were removed to satisfy the output bound. |
| `dataset_version`, `retriever_version` | Reproducibility identifiers. |

An empty result list is a successful search outcome. Invalid input or worker failure is a typed error, never disguised as an empty search.

## Storage boundary

```text
permitted raw assets and payloads    content-addressed local files      Phase 1
canonical metadata and FTS           SQLite                            Phase 1
active corpus                         checksummed manifest pointer       Phase 1
TF-IDF/LSA experiment artifacts      NumPy matrices + ordered JSON IDs  Phase 2
benchmark and reports                 versioned JSON/Markdown            Phase 2
selection feedback                    optional local JSONL               Phase 3
source plan/cursor/raw/report         planned versioned acquisition files Stage 4
```

Raw, canonical, and derived data are layers of one corpus, not three competing sources of truth. Derived FTS and embedding artifacts can be rebuilt. A model change produces a new manifest and never overwrites the artifacts attached to an earlier score.

## Repository shape

The paused Phase 3 release keeps this single-package shape:

```text
README.md
docs/
Cargo.toml
Cargo.lock
rust-toolchain.toml
migrations/
    001_initial.sql
src/
    main.rs                  ingest, TUI, feedback, and profile dispatch
    ingest.rs                local validation, storage, and outcomes
    protocol.rs              strict Rust NDJSON message types
    worker.rs                bounded Python process and response validation
    tui.rs                   state, keys, result/preview rendering, and help
    interface.rs             terminal guard, controller, actions, and lifecycle
    feedback.rs              opt-in private JSONL events and retention
    profile.rs               release-mode PTY submit-to-render profile
pyproject.toml
python/mambomeme_search/
    build_index.py           deterministic FTS/LSA build and publication
    dense.py                 TF-IDF/LSA artifact build and exact cosine scan
    retrieve.py              validation, routes, fusion, and result contract
    worker.py                long-lived strict NDJSON process
    evaluate.py              metrics, comparison, and report generation
    text.py                  shared search normalization and tokenization
benchmarks/
    provisional-v1.json
    reports/phase2-provisional.{json,md}
    reports/phase3-interface.json
tests/
    fixtures/corpus/         cleared local manifest and assets
    fixtures/protocol/       cross-language golden messages
    python/                  index, retrieval, worker, and evaluator tests
    test_phase1.py           offline Rust-to-Python acceptance check
    test_phase2.py           offline corpus-to-worker acceptance check
    test_phase3.py           offline PTY selection, feedback, failure, and profile
data/                        ignored working and published artifacts
```

Stage 4 may add one concrete `commons.rs` source module, a reviewed plan example, one offline acceptance script, and one source report only when the complete stage closes. One Cargo package and one Python package remain enough; do not add a Cargo workspace, web service, message queue, vector service, generic adapter framework, or empty context package.

## Failure behavior

| Failure | Required behavior |
|---|---|
| Invalid or rights-incomplete source item | Quarantine the item without losing the batch. |
| Planned Commons content, identity, dimension, or rights drift | Stage 4 must quarantine the new revision and remove a previously served revision until it is reviewed. |
| Planned explicit Commons deletion or completed-plan removal | Stage 4 must record deletion, invalidate serving state, then rebuild before serving. |
| Planned API omission, interruption, or exhausted transient failure | Stage 4 must preserve prior availability and never infer a deletion. |
| Rust/Python schema mismatch | Stop the build or worker startup. |
| Corrupt manifest or item/vector mismatch | Refuse publication or startup. |
| Dense artifact missing or corrupt at startup | Reject the complete Phase 2 snapshot; lexical-only degradation is allowed only for a runtime dense-route failure after successful startup. |
| FTS unavailable in a scored run | Fail that run; do not silently change the evaluated system. |
| Worker crash or timeout | Preserve terminal control, show an error, and allow an explicit restart. |
| Unsupported terminal image protocol | Use the text/metadata preview and external-open action. |
| No eligible match | Return an empty result list. |
| Future permission revocation or active-pointer change | Stage 4 must stop the open worker and serving snapshot until a replacement is built and the worker restarts. |

## Deferred boundary

The future context assistant may accept one sentence or a bounded sequence of role-labelled chat messages. It will derive several short search cues and pass them through the same retrieval contract. Its model, privacy rules, prompt-injection handling, and evaluation belong to a separate benchmark and are not part of the first repository implementation.
