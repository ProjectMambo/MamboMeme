---
description: A Rust and Python application for processing, searching, previewing, and selecting meme images and quotes.
title: MamboMeme
order: 65
---

::page{layout="project" width="normal" sidebar=true}

# MamboMeme

MamboMeme is a local ranked-search application for meme images and quotes. Rust owns ingestion and the future terminal interface; Python owns index construction, ranked retrieval, the long-lived worker, and evaluation.

## Project boundary

- A query such as `john cena` returns ranked stored assets for the user to inspect and choose.
- Phase 1 builds a deterministic local SQLite/FTS corpus from a cleared fixture.
- Phase 2 implements BM25, a measured LSA dense experiment, deterministic hybrid fusion, evaluation, and the worker. BM25 remains the default because fusion did not pass the confidence rule.
- Phase 3 adds a Rust TUI connected to one long-lived Python retrieval worker through a versioned local protocol.
- Phase 4 adds one approved external source through the same corpus contract.
- Selection feedback is optional offline evidence, never live self-training.
- Context-aware suggestions reuse retrieval only after the prompt-search product works.
- MMTS-Search-v1, component metrics, and contract tests justify each implemented stage.

## Documentation

::children{view="list" sort="order" direction="asc" show=["title","description"]}

## Project status

Phases 1 and 2 are complete. The bounded local corpus, checksummed FTS5/LSA snapshot, ranked-search routes, evaluator, and strict NDJSON worker pass their offline checks. Phase 3 is the Rust TUI and first-release score; no external source, context assistant, public API, or deployment target is implemented yet.
