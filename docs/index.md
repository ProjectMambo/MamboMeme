---
description: A Rust and Python application for processing, searching, previewing, and selecting meme images and quotes.
title: MamboMeme
order: 65
---

::page{layout="project" width="normal" sidebar=true}

# MamboMeme

MamboMeme is a local ranked-search application for meme images and quotes. Rust owns ingestion and the future terminal interface; Python owns index construction, future ranked retrieval, and evaluation.

## Project boundary

- A query such as `john cena` returns ranked stored assets for the user to inspect and choose.
- Phase 1 builds a deterministic local SQLite/FTS corpus from a cleared fixture.
- Phase 2 adds BM25 for exact names, templates, quotes, and tags, then keeps dense text only if it measurably helps descriptions and concepts.
- Phase 3 adds a Rust TUI connected to one long-lived Python retrieval worker through a versioned local protocol.
- Phase 4 adds one approved external source through the same corpus contract.
- Selection feedback is optional offline evidence, never live self-training.
- Context-aware suggestions reuse retrieval only after the prompt-search product works.
- MMTS-Search-v1, component metrics, and contract tests justify each implemented stage.

## Documentation

::children{view="list" sort="order" direction="asc" show=["title","description"]}

## Project status

Phase 1 is complete. The Rust command ingests and deduplicates bounded local records, the Python builder publishes a deterministic checksummed FTS5 snapshot, and their offline acceptance checks pass. No retrieval worker, dense model, TUI, external source, context assistant, public API, or deployment target is implemented yet.
