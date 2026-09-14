# MamboMeme

<p align="left">
  <img src="https://img.shields.io/badge/Status-Design-4c8bf5?style=flat-square" alt="Project status: design" />
</p>

MamboMeme is a planned local search application for finding existing meme images and quotes. A user searches for a name, quote, topic, or description—such as `john cena`—reviews a ranked list, and chooses the item they want.

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
| Data processing and storage | Core | Acquire permitted meme records, validate and deduplicate them, enrich searchable fields, and publish a versioned corpus. |
| Retrieval | Core | Search by exact wording, people, templates, tags, visual descriptions, or semantic concepts and return ranked matches. |
| User interface | Core | Let a user search, inspect, navigate, and select results through a Rust terminal interface. |
| Context-aware suggestions | Future | Convert one message or a short chat into search cues, then reuse the same retrieval engine. |

## System at a glance

```text
OFFLINE CORPUS BUILD
permitted source
    -> Rust acquisition, validation, hashing, and staging
    -> Python OCR, annotations, and embeddings
    -> versioned SQLite, vector, and manifest artifacts

INTERACTIVE SEARCH
query such as "john cena"
    -> Rust TUI
    -> long-lived Python retrieval worker
    -> BM25 lexical search + dense semantic search
    -> ranked meme images and quotes
    -> user previews and selects one result
```

The TUI and Python worker exchange versioned newline-delimited JSON over one local subprocess session. The model and index load once; Rust does not duplicate Python retrieval logic.

## Why hybrid search

`john cena` should match names, template labels, tags, and OCR text through lexical search. A query such as `the wrestler you cannot see` may need semantic retrieval because the useful item need not contain the same words.

- SQLite FTS5/BM25 handles exact names, quotes, tags, and rare terms.
- Dense text retrieval handles paraphrases, concepts, and visual descriptions.
- Rank fusion combines both candidate lists without pretending their raw scores are comparable.
- Image embeddings remain a measured later experiment.

Semantic search stays only if the benchmark shows a meaningful gain over BM25 without harming exact-name and exact-quote search.

## Selection feedback

The interface may record local, opt-in events such as which ranked item was opened or selected. These events can diagnose findability and later supply reviewed training examples, but they are not ground truth: result position and presentation strongly influence selection.

There is no live self-training in the first release. Official quality scores always come from a frozen human-labelled benchmark.

## Current status

MamboMeme is documentation only. The repository contains synchronized design documents, not an application, corpus, model, dependency set, licence, public API, or deployment target.
