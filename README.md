# MamboMeme

<p align="left">
  <img src="https://img.shields.io/badge/Status-Phase_2_complete-2ea44f?style=flat-square" alt="Project status: Phase 2 complete" />
</p>

MamboMeme is a local search application for finding existing meme images and quotes. A user searches for a name, quote, topic, or description—such as `john cena`—reviews a ranked list, and chooses the item they want.

It is a **ranked multimodal search project**, not a reply generator in its first release. The ML work is retrieval: building useful representations, comparing lexical and semantic routes, ranking results, and measuring whether the desired meme is easy to find.

## Start here

| Goal | Document |
|---|---|
| Understand the components and Rust/Python boundaries | [Architecture](docs/Architecture.md) |
| Plan acquisition, parsing, enrichment, and storage | [Data pipeline](docs/Data%20Pipeline.md) |
| Follow a query through ranking and selection | [Retrieval](docs/Retrieval.md) |
| Understand the planned Rust TUI | [Interface](docs/Interface.md) |
| See the complete correctness test matrix | [Testing](docs/Testing.md) |
| Understand metrics and the Technical Score | [Evaluation](docs/Evaluation.md) |
| Follow the implementation order | [Roadmap](docs/Roadmap.md) |

## Four parts

| Part | Status | Purpose |
|---|---|---|
| Data processing and storage | Phase 1 complete | Validate and deduplicate a cleared local fixture, preserve provenance, build fielded FTS5, and publish a versioned corpus. |
| Retrieval | Phase 2 complete | Search by exact wording, people, templates, tags, visual descriptions, or semantic concepts and return ranked matches. |
| User interface | Phase 3 planned | Let a user search, inspect, navigate, and select results through a Rust terminal interface. |
| Context-aware suggestions | Future | Convert one message or a short chat into search cues, then reuse the same retrieval engine. |

## System at a glance

```text
PHASE 1  cleared local manifest -> Rust ingest -> SQLite -> Python FTS snapshot
PHASE 2  query -> Python BM25 / measured dense experiment -> ranked results
PHASE 3  Rust TUI -> long-lived Python worker -> preview and explicit selection
PHASE 4  one approved external source -> the same corpus contract
FUTURE   bounded chat context -> search cues -> the same retrieval contract
```

The first three phases complete the local search product. Phase 4 proves repeatable real-source ingestion. Phase 2 implements the Python worker and the shared newline-delimited JSON protocol; Phase 3 will add the Rust TUI client.

## Why evaluate hybrid search

`john cena` should match names, template labels, tags, and OCR text through lexical search. A query such as `the wrestler you cannot see` may need semantic retrieval because the useful item need not contain the same words.

- Phase 2 implements weighted SQLite FTS5/BM25 for exact names, quotes, tags, and rare terms.
- Its measured dense experiment uses deterministic TF-IDF plus truncated SVD and exact cosine search; it is a lightweight text baseline, not an image model.
- Reciprocal-rank fusion compares the routes without treating raw BM25 and cosine values as comparable.
- Image embeddings remain a measured later experiment.

The public fixture comparison did not satisfy the confidence rule, so lexical search remains the default. The dense and hybrid routes stay available for explicit experiments and regression tests.

## Selection feedback

The interface may record local, opt-in events such as which ranked item was opened or selected. These events can diagnose findability and later supply reviewed training examples, but they are not ground truth: result position and presentation strongly influence selection.

There is no live self-training in the first release. Official quality scores always come from a frozen human-labelled benchmark.

## Current status

Phases 1 and 2 are complete. The repository contains the local corpus pipeline, weighted lexical search, a small LSA dense experiment, deterministic hybrid ranking, a strict long-lived NDJSON worker, a public provisional benchmark, and offline acceptance checks. The commands in [Testing](docs/Testing.md#implemented-commands) pass from a clean working tree.

The provisional report records excellent fixture retrieval but deliberately leaves MMTS-Search-v1 unscored: the human-labelled safety subset and Rust TUI submit-to-render latency do not exist yet. There is no TUI, external crawler, context assistant, public API, or deployment target. See the four substantial delivery phases in the [Roadmap](docs/Roadmap.md).
