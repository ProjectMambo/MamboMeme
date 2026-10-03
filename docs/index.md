---
description: A Rust and Python application for processing, searching, previewing, and selecting meme images and quotes.
title: MamboMeme
order: 65
---

::page{layout="project" width="normal" sidebar=true}

# MamboMeme

MamboMeme is a local ranked-search application for meme images and quotes. Rust owns ingestion and the terminal interface; Python owns index construction, ranked retrieval, the long-lived worker, and evaluation.

## Project boundary

- A query such as `john cena` returns ranked stored assets for the user to inspect and choose.
- Phase 1 builds a deterministic local SQLite/FTS corpus from a cleared fixture.
- Phase 2 implements BM25, a measured LSA dense experiment, deterministic hybrid fusion, evaluation, and the worker. BM25 remains the default because fusion did not pass the confidence rule.
- Phase 3 implements a Rust TUI connected to one long-lived Python retrieval worker through a versioned local protocol.
- Phase 4 is adding one finite, human-reviewed Wikimedia Commons page-ID plan through the same corpus contract; it is not a general crawler.
- Selection feedback is optional offline evidence, never live self-training.
- Context-aware suggestions reuse retrieval only after the prompt-search product works.
- MMTS-Search-v1, component metrics, and contract tests justify each implemented stage.

## Documentation

::children{view="list" sort="order" direction="asc" show=["title","description"]}

## Project status

Phases 1 through 3 are complete. Phase 4's reviewed Commons source, tombstone replay, fail-closed snapshot invalidation, and controlled network tests are in progress. MMTS-Search-v1 remains unscored until an independent human-labelled hidden benchmark with its safety subset exists; no context assistant, public API, packaged distribution, or deployment target is implemented.
