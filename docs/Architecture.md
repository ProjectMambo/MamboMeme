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

Delivery uses four implementation phases rather than one phase per product component: Phase 1 creates an executable local corpus, Phase 2 creates ranked retrieval and evaluation, Phase 3 creates the TUI and first release, and Phase 4 adds one approved external source. See the [Roadmap](Roadmap.md).

## Language ownership

The full system keeps one owner for each responsibility:

| Rust application | Python search package |
|---|---|
| Source acquisition, first-pass trust-boundary validation, hashing, provenance, exact deduplication, staging, terminal lifecycle, TUI state, result presentation, and optional local interaction-event capture | Resource-limited OCR and annotations, search-document construction, embeddings, FTS and dense retrieval, rank fusion, filters, evaluation, and the long-lived search worker |

Rust is useful for a distributable terminal program, careful handling of source bytes, and systems practice. Python keeps the ML ecosystem close to the model and evaluation code. This split is not a blanket claim that Rust makes every operation faster:

- SQLite FTS executes inside SQLite whichever language submits the query.
- NumPy and model frameworks already perform heavy numeric work in native kernels.
- Model encoding is likely to dominate search latency on a small local corpus.

Keep one implementation of each responsibility. Measure before moving a boundary.

## Current implementation boundary

Phases 1 and 2 are complete. Phase 1 established the durable corpus boundary:

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

The initial migration, cleared fixture data, Rust command, Python builder, retrieval routes, evaluator, protocol types, worker, unit tests, and offline cross-language fixtures are implemented. Local image ingestion still accepts PPM fixture images only; broader decoding belongs with real-source ingestion.

There are still no network requests, OCR calls, pretrained model downloads, image embeddings, terminal rendering, feedback records, or context processing. Those boundaries belong to later phases and must not be inferred from the presence of their design documents.

## Full offline corpus build

```text
approved API or cleared local manifest
    -> Rust fetches or reads, validates, hashes, deduplicates, and stages
    -> raw content-addressed assets + SQLite provenance rows
    -> Python performs constrained OCR and enrichment only when a source needs them
    -> Python builds fielded FTS documents and declared dense representations
    -> Python validates and atomically publishes a corpus snapshot
```

The stages do not write concurrently. Rust completes its acquisition transaction before Python enrichment begins. The interactive search worker opens only the published snapshot and treats it as read-only.

Phase 2 adds a model-free TF-IDF/LSA representation and retrieval. Pretrained text/image models remain later measured experiments; remote acquisition begins only in Phase 4.

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

Phase 2 creates the worker and headless retrieval. Phase 3 starts one worker per TUI session so the index and corpus load once. There is no HTTP server, embedded Python, per-query process, or duplicate Rust search implementation in the first release.

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

Phase 3's Rust client will enforce startup/query timeouts and one in-flight submission in the UI. A mismatched or stale request ID never replaces the displayed result set. The TUI may offer one deliberate worker restart; it must not loop indefinitely.

## Artifact contract

The table describes the full release contract. Phases 1 and 2 implement the raw-media, SQLite, migration, published-manifest, FTS, and LSA rows; interaction events arrive only in Phase 3.

Rust and Python exchange durable, inspectable build artifacts:

| Artifact | Owner | Consumer |
|---|---|---|
| Content-addressed raw media | Rust writes | Python reads within the enrichment sandbox |
| Source, rights, outcome, and processing rows in SQLite | Rust writes | Python enriches during a stopped build |
| Ordered, language-neutral SQL migrations | Shared contract | Both apply or inspect |
| Field-labelled search documents and FTS tables | Python writes | Python worker reads |
| `dense_ids.json`, vocabulary/IDF files, SVD components, and L2-normalized dense vectors | Python writes | Python worker reads |
| Published manifest and checksums | Python writes | Python worker verifies at startup |
| Optional interaction events in versioned JSON Lines | Rust writes | Offline Python analysis reads |

The manifest records schema, builder, normalization, search-document, representation, NumPy and SQLite versions; canonical content identity; dense method and dimension; item/vocabulary counts; and every artifact checksum. Corpus identity is computed from a canonical ordered export rather than SQLite page bytes or timestamps. The snapshot identity also protects the derived artifact manifest.

The Phase 1 portion of one fixed fixture must prove ingestion, outcomes, publication, and reproducibility. Phases 2 and 3 extend the same fixture through retrieval and selection:

```text
Rust fixture import
    -> expected accepted, duplicate, and quarantine outcomes
    -> Python FTS index build and atomic publication
    -> Phase 2 worker returns the development-only "john cena" fixture at rank 1
    -> both languages decode the same golden protocol messages
    -> a second build preserves content identity and ranking
    -> Phase 3 Rust TUI receives and selects the intended stable ID
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
```

Raw, canonical, and derived data are layers of one corpus, not three competing sources of truth. Derived FTS and embedding artifacts can be rebuilt. A model change produces a new manifest and never overwrites the artifacts attached to an earlier score.

## Repository shape by phase

Phases 1 and 2 use this single-package shape:

```text
README.md
docs/
Cargo.toml
Cargo.lock
rust-toolchain.toml
migrations/
    001_initial.sql
src/
    main.rs                  command entry point and shared protocol module
    ingest.rs                local validation, storage, and outcomes
    protocol.rs              strict Rust NDJSON message types
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
tests/
    fixtures/corpus/         cleared local manifest and assets
    fixtures/protocol/       cross-language golden messages
    python/                  index, retrieval, worker, and evaluator tests
    test_phase1.py           offline Rust-to-Python acceptance check
    test_phase2.py           offline corpus-to-worker acceptance check
data/                        ignored working and published artifacts
```

Phase 3 adds the TUI and interaction-event files. Phase 4 adds one concrete source integration. One Cargo package and one Python package remain enough; do not add a Cargo workspace, web service, message queue, vector service, generic adapter framework, or empty context package.

## Failure behavior

| Failure | Required behavior |
|---|---|
| Invalid or rights-incomplete source item | Quarantine the item without losing the batch. |
| Rust/Python schema mismatch | Stop the build or worker startup. |
| Corrupt manifest or item/vector mismatch | Refuse publication or startup. |
| Dense artifact missing or corrupt at startup | Reject the complete Phase 2 snapshot; lexical-only degradation is allowed only for a runtime dense-route failure after successful startup. |
| FTS unavailable in a scored run | Fail that run; do not silently change the evaluated system. |
| Worker crash or timeout | Preserve terminal control, show an error, and allow an explicit restart. |
| Unsupported terminal image protocol | Use the text/metadata preview and external-open action. |
| No eligible match | Return an empty result list. |
| Permission revocation | Stop serving the affected snapshot until it is rebuilt without the item. |

## Deferred boundary

The future context assistant may accept one sentence or a bounded sequence of role-labelled chat messages. It will derive several short search cues and pass them through the same retrieval contract. Its model, privacy rules, prompt-injection handling, and evaluation belong to a separate benchmark and are not part of the first repository implementation.
