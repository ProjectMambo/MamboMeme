# MamboMeme

<p align="left">
  <img src="https://img.shields.io/badge/Status-Phase_3_complete-2ea44f?style=flat-square" alt="Project status: Phase 3 complete" />
</p>

MamboMeme is a local search application for finding existing meme images and quotes. A user searches for a name, quote, topic, or description—such as `john cena`—reviews a ranked list, and chooses the item they want.

It is a **ranked multimodal search project**, not a reply generator in its first release. The ML work is retrieval: building useful representations, comparing lexical and semantic routes, ranking results, and measuring whether the desired meme is easy to find.

## Start here

| Goal | Document |
|---|---|
| Understand the components and Rust/Python boundaries | [Architecture](Architecture.md) |
| Plan acquisition, parsing, enrichment, and storage | [Data pipeline](Data%20Pipeline.md) |
| Follow a query through ranking and selection | [Retrieval](Retrieval.md) |
| Use and understand the Rust TUI | [Interface](Interface.md) |
| See the complete correctness test matrix | [Testing](Testing.md) |
| Understand metrics and the Technical Score | [Evaluation](Evaluation.md) |
| Follow the implementation order | [Roadmap](Roadmap.md) |

## Four parts

| Part | Status | Purpose |
|---|---|---|
| Data processing and storage | Phase 1 complete | Validate and deduplicate a cleared local fixture, preserve provenance, build fielded FTS5, and publish a versioned corpus. |
| Retrieval | Phase 2 complete | Search by exact wording, people, templates, tags, visual descriptions, or semantic concepts and return ranked matches. |
| User interface | Phase 3 complete | Search, inspect, navigate, open, manually copy, and select results through a Rust terminal interface. |
| Context-aware suggestions | Future | Convert one message or a short chat into search cues, then reuse the same retrieval engine. |

## System at a glance

```text
PHASE 1  cleared local manifest -> Rust ingest -> SQLite -> Python FTS snapshot
PHASE 2  query -> Python BM25 / measured dense experiment -> ranked results
PHASE 3  Rust TUI -> long-lived Python worker -> preview and explicit selection
PHASE 4  one approved external source -> the same corpus contract
FUTURE   bounded chat context -> search cues -> the same retrieval contract
```

The first three phases complete the local search product. Phase 4 proves repeatable real-source ingestion. Phase 2 implements the Python worker and shared newline-delimited JSON protocol; Phase 3 connects the Rust TUI client, terminal lifecycle, feedback log, and interface profiler.

## Run the source release

From the repository root, build the fixture corpus and launch the TUI:

```sh
DATA_DIR=/tmp/mambomeme-data
cargo run --locked -- ingest --manifest tests/fixtures/corpus/manifest.jsonl --data-dir "$DATA_DIR"
PYTHONPATH=python python3 -m mambomeme_search.build_index --data-dir "$DATA_DIR"
cargo run --locked -- tui --data-dir "$DATA_DIR"
```

Add `--feedback` to the `tui` command only when local interaction logging is wanted. The [Interface](Interface.md) document covers keys, feedback inspection/deletion, the release build, and the reproducible interface profile.

## Why evaluate hybrid search

`john cena` should match names, template labels, tags, and OCR text through lexical search. A query such as `the wrestler you cannot see` may need semantic retrieval because the useful item need not contain the same words.

- Phase 2 implements weighted SQLite FTS5/BM25 for exact names, quotes, tags, and rare terms.
- Its measured dense experiment uses deterministic TF-IDF plus truncated SVD and exact cosine search; it is a lightweight text baseline, not an image model.
- Reciprocal-rank fusion compares the routes without treating raw BM25 and cosine values as comparable.
- Image embeddings remain a measured later experiment.

The public fixture comparison did not satisfy the confidence rule, so lexical search remains the default. The dense and hybrid routes stay available for explicit experiments and regression tests.

## Selection feedback

The interface can record local, opt-in events such as open, copy, choose, reformulate, and abandon. These events can diagnose findability and later supply reviewed training examples, but they are not ground truth: result position and presentation strongly influence selection.

There is no live self-training in the first release. Official quality scores always come from a frozen human-labelled benchmark.

## Current status

Phases 1 through 3 are complete. The repository contains the local corpus pipeline, weighted lexical search, a small LSA dense experiment, deterministic hybrid ranking, a strict long-lived NDJSON worker, the Rust TUI, optional private local feedback, a public provisional benchmark, and offline acceptance checks. The commands in [Testing](Testing.md#implemented-commands) pass from a clean working tree.

The Phase 3 interface profile records `11.377 ms` p95 submit-to-completed-result-state latency across 1,024 measured searches with zero request errors on one declared release-mode `80×24` PTY run. MMTS-Search-v1 still has a null score and `INCOMPLETE` status because the independent human-labelled hidden benchmark with its safety subset does not exist. There is no external crawler, context assistant, public API, packaged binary distribution, or deployment target. Phase 4 is next in the [Roadmap](Roadmap.md).
