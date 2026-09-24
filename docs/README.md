# MamboMeme

<p align="left">
  <img src="https://img.shields.io/badge/Status-Phase_1_complete-2ea44f?style=flat-square" alt="Project status: Phase 1 complete" />
</p>

MamboMeme is a local search application for finding existing meme images and quotes. A user searches for a name, quote, topic, or description—such as `john cena`—reviews a ranked list, and chooses the item they want.

It is a **ranked multimodal search project**, not a reply generator in its first release. The ML work is retrieval: building useful representations, comparing lexical and semantic routes, ranking results, and measuring whether the desired meme is easy to find.

## Start here

| Goal | Document |
|---|---|
| Understand the components and Rust/Python boundaries | [Architecture](Architecture.md) |
| Plan acquisition, parsing, enrichment, and storage | [Data pipeline](Data%20Pipeline.md) |
| Follow a query through ranking and selection | [Retrieval](Retrieval.md) |
| Understand the planned Rust TUI | [Interface](Interface.md) |
| See the complete correctness test matrix | [Testing](Testing.md) |
| Understand metrics and the Technical Score | [Evaluation](Evaluation.md) |
| Follow the implementation order | [Roadmap](Roadmap.md) |

## Four parts

| Part | Status | Purpose |
|---|---|---|
| Data processing and storage | Phase 1 complete | Validate and deduplicate a cleared local fixture, preserve provenance, build fielded FTS5, and publish a versioned corpus. |
| Retrieval | Phase 2 planned | Search by exact wording, people, templates, tags, visual descriptions, or semantic concepts and return ranked matches. |
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

The first three phases complete the local search product. Phase 4 proves repeatable real-source ingestion. The TUI and Python worker will exchange versioned newline-delimited JSON over one local subprocess session; neither exists in Phase 1.

## Why evaluate hybrid search

`john cena` should match names, template labels, tags, and OCR text through lexical search. A query such as `the wrestler you cannot see` may need semantic retrieval because the useful item need not contain the same words.

- Phase 1 builds deterministic SQLite FTS5 data; Phase 2 turns it into the BM25 baseline for exact names, quotes, tags, and rare terms.
- A Phase 2 dense experiment will test paraphrases, concepts, and visual descriptions.
- Rank fusion is added only for that measured comparison and does not treat raw BM25 and dense scores as comparable.
- Image embeddings remain a measured later experiment.

Semantic search stays only if the benchmark shows a meaningful gain over BM25 without harming exact-name and exact-quote search.

## Selection feedback

The interface may record local, opt-in events such as which ranked item was opened or selected. These events can diagnose findability and later supply reviewed training examples, but they are not ground truth: result position and presentation strongly influence selection.

There is no live self-training in the first release. Official quality scores always come from a frozen human-labelled benchmark.

## Current status

Phase 1 is complete. The repository contains the initial SQLite migration, a cleared fixture corpus, the Rust local ingester, the deterministic Python FTS snapshot builder, and an offline cross-language check. The commands in [Testing](Testing.md#phase-1-commands) pass from a clean working tree.

There is no retrieval worker, dense model, TUI, external crawler, context assistant, public API, or deployment target yet. See the four substantial delivery phases in the [Roadmap](Roadmap.md).
