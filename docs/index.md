---
description: A planned Rust and Python application for processing, searching, previewing, and selecting meme images and quotes.
title: MamboMeme
order: 65
---

::page{layout="project" width="normal" sidebar=true}

# MamboMeme

MamboMeme is a local ranked-search application for meme images and quotes. Rust will handle ingestion and the terminal interface; Python will build the search index, run lexical and semantic retrieval, rank results, and evaluate quality.

## Project boundary

- A query such as `john cena` returns ranked stored assets for the user to inspect and choose.
- BM25 handles exact names, templates, quotes, and tags; dense text covers descriptions and concepts.
- A Rust TUI talks to one long-lived Python retrieval worker through a versioned local protocol.
- Selection feedback is optional offline evidence, never live self-training.
- Context-aware suggestions reuse retrieval only after the prompt-search product works.
- MMTS-Search-v1, component metrics, and contract tests justify each implemented stage.

## Documentation

::children{view="list" sort="order" direction="asc" show=["title","description"]}

## Project status

The project repository is currently a documentation-only snapshot. The corpus, implementation, model, public remote, licence, and deployment surface have not been created.
