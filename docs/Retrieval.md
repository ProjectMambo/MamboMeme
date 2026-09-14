---
description: Query semantics, lexical and dense retrieval, rank fusion, filtering, result contracts, and selection feedback.
title: Retrieval
order: 30
---

::page{layout="docs" width="normal" sidebar=true}

# Retrieval

Retrieval turns a query into a ranked list of existing meme items. The user decides which result is useful; the engine does not compose a reply.

## Query contract

The retrieval worker accepts one query string or several alternate search cues. Each cue may describe:

- a person or character: `john cena`;
- a template or title: `distracted boyfriend`;
- a quote or OCR fragment: `you can't see me`;
- a topic or tag: `wrestling`;
- a visual scene: `wrestler waving hand in front of face`;
- a concept: `pretending to be invisible`;
- noisy text: `jon sena invisible meme`.

Multiple cues are OR-style hints. Retrieve each independently and fuse the ranks so one weak cue cannot erase a useful exact match. They are not chat messages and are not averaged into one embedding.

Normalize corpus fields and queries with the same versioned function: Unicode NFKC plus whitespace collapse. Preserve original strings for display. Bound cue count, UTF-8 byte length, result limit, and filter values before storage or model work.

## Search fields

| Field | Best for | Priority |
|---|---|---:|
| Title and aliases | Known meme names and common variants | Highest |
| People and characters | `john cena`, `drake`, `pikachu` | Highest |
| Template | Template identity and grouping | High |
| Quote and corrected OCR | Exact visible wording | High |
| Tags | Topics, slang, franchises, actions | Medium |
| Literal caption | Visual descriptions | Medium |
| Search description | Concepts and paraphrases | Medium |
| Raw OCR | Recall when correction is unavailable | Low |

Generated descriptions must not bury exact identity evidence. Keep fields separate for lexical weights and diagnostics even when the dense route uses a combined field-labelled document.

## Lexical route

SQLite FTS5/BM25 is the mandatory baseline. It should make `john cena` strongly match title, alias, people, template, tags, and OCR fields.

The query boundary treats input as plain text, not user-authored FTS grammar:

1. tokenize bounded normalized text;
2. escape and quote every literal token or phrase;
3. build only the supported internal expression;
4. bind that expression as a SQL parameter;
5. apply declared column weights and deterministic tie-breaking.

Do not expose raw `MATCH` operators. Quotes, parentheses, hyphens, Unicode, and empty input are explicit tests.

## Semantic route

Dense text retrieval covers meaning that lexical terms miss. It encodes the query and a field-labelled searchable representation into the same vector space and performs cosine search over L2-normalized vectors.

For the initial corpus, exact NumPy matrix multiplication is enough. A vector database or approximate-nearest-neighbour index is justified only after the real corpus misses its latency target.

The dense representation includes identity fields as well as captions and descriptions. This lets `the wrestler you cannot see` retrieve a John Cena item while still giving the lexical route responsibility for the direct `john cena` query.

The first implementation is dense **text-to-text** retrieval. CLIP-style image embeddings are a separate later experiment because their contribution must be measured independently.

## Hybrid ranking

Construct the static and request-specific eligibility mask before ranking: published, available, licensed, safe, and compatible with the requested kind/language filters. Each route searches only that mask. If a backend cannot mask directly, it must over-fetch and refill deterministically until it finds the required eligible depth or exhausts its candidates.

Retrieve a fixed eligible candidate depth from each active cue and route. Fuse their ranks using weighted reciprocal-rank fusion:

```text
RRF(item) =
  sum(cue_weight * route_weight / (k + route_rank))
  -------------------------------------------------
  sum(cue_weight * route_weight for active routes)
```

V1 uses equal cue weights. Route weights, candidate depth, `k`, and the minimum-evidence threshold are selected on development data and frozen before the hidden benchmark.

After fusion:

1. join canonical metadata and assert every item still satisfies the eligibility mask;
2. collapse exact duplicates;
3. cap repeated variants from one template group;
4. refill from the fused eligible candidates after duplicate/template removal;
5. sort by fused rank, then stable item ID for deterministic ties;
6. return up to the requested limit or exhaust eligible candidates.

Raw BM25, cosine, and fused values are evidence, not probabilities. An empty list is correct when no eligible candidate has sufficient evidence.

## Baselines and ablations

Evaluate these systems on the same corpus and benchmark:

1. lexical BM25 only;
2. dense text only;
3. lexical plus dense fusion;
4. hybrid without each major field in turn;
5. image-vector route only when implemented;
6. reranker only after reviewed hard negatives exist.

The hybrid system becomes the default only if it improves semantic/concept searches without materially regressing entity, name, template, or exact-quote searches.

## Result presentation

Every ranked result includes enough information for the user and tests to understand it:

| Value | Use |
|---|---|
| Stable ID and rank | Selection, events, reproducibility, and deterministic tests |
| Title, item type, and text | Compact result row |
| Thumbnail or asset URI | Preview and external open |
| People, template, and tags | Identity and quick verification |
| Caption | Portable fallback when inline image display is unavailable |
| Source and attribution | Rights-compliant display |
| Matched fields and routes | Debugging and optional UI explanation |
| Dataset and retriever versions | Reproduce the ranking |

## Interaction feedback

An `interaction_event` may be recorded locally when the user explicitly enables feedback. Its action is one of `preview`, `open`, `copy`, `choose`, `reformulate`, or `abandon`. Only `choose` is the v1 success signal; the other actions are diagnostic and must never be silently relabelled as a selection.

The initial privacy contract is:

- logging is off by default, and disabling it stops new writes immediately;
- events stay in one owner-readable/writable local file with mode `0600` where the platform supports it;
- raw normalized query text is stored only after opt-in because reformulation analysis requires it; hashing a low-entropy query is not anonymization;
- one random session ID is created per launch and is never reused as a user identity;
- events expire after 30 days by default and the user can delete the file immediately;
- no event contains chat context, clipboard contents, machine identity, or a stable user ID;
- nothing is uploaded without a separate explicit export/upload action and consent.

Minimum fields are:

- schema version, random event ID, random launch-scoped session ID, and UTC timestamp;
- request ID and raw normalized query text under the retention policy;
- ordered IDs and ranks that were actually shown;
- action plus target item ID and shown rank when that action targets an item;
- dataset, retriever, and interface versions;
- elapsed time since the associated result list was completely rendered.

Selection is biased by rank, exposure, popularity, familiarity, and preview quality. Therefore:

- report `choose` and the diagnostic actions as observational product data, not benchmark truth;
- do not update live rankings directly from raw selections;
- use reviewed selections for hard-negative analysis first;
- require randomized or interleaved exposure with logged propensities before causal click-learning claims;
- gate every later learned ranker against the frozen human-labelled benchmark.

## Scaling triggers

| Add | Only when |
|---|---|
| Approximate vector index | Exact dense search fails the real p95 latency gate. |
| Vector database | Concurrent remote clients, shared writes, or filtering requirements exceed local artifacts. |
| Learned reranker | Enough reviewed query-item pairs and hard negatives exist. |
| Image embeddings | The visual-description slice shows a measured text-only gap. |
| PyO3 | Profiling finds a fine-grained CPU hotspot that batching or native libraries cannot remove. |
| Network service | The Rust interface and Python worker must deploy or scale independently. |
