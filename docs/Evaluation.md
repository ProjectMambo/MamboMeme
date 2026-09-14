---
description: Search benchmark, relevance and performance metrics, selection diagnostics, Technical Score, gates, and comparison protocol.
title: Evaluation
order: 60
---

::page{layout="docs" width="normal" sidebar=true}

# Evaluation

MamboMeme is evaluated as ranked search. Quality means that useful assets appear early for the user's query, exact identities remain findable, irrelevant nearest neighbours can be withheld, and the full interface responds reliably.

## Benchmark v1

Create two versioned partitions with no shared query family or meme template/perceptual group:

| Partition | Answerable | No-match | Use |
|---|---:|---:|---|
| Development | 90 | 30 | Choose fields, weights, fusion constants, thresholds, and model |
| Hidden test | 180 | 60 | Run once for the frozen release comparison |

Answerable prompts are balanced across six primary slices:

1. person, character, entity, or named template;
2. exact quote or OCR fragment;
3. topic, tag, franchise, or action;
4. semantic concept or paraphrase without lexical overlap;
5. literal visual description;
6. typo, slang, casing, punctuation, or other noisy text.

The development set contains 15 queries per slice; the hidden set contains 30 per slice. Cross-cutting subsets mark multi-cue/filter queries and safety/adversarial queries without increasing the totals. The hidden safety subset contains 30 answerable and 10 no-match queries; the development subset contains 15 and 5.

`john cena` belongs to the public fixture and development examples only. The hidden partition uses unseen entity/template families so its answer is not encoded in tests or documentation.

Paraphrases, aliases, and near-identical variants belong to the same query family. They cannot leak across partitions. Once hidden prompts or labels are exposed, a later release needs a new hidden benchmark version and must rerun every baseline on that version.

A smaller initial benchmark is allowed only with a **provisional** label. Context-aware suggestions have a different task and are excluded from benchmark v1.

## Relevance labels

Human judges grade whether each stored item is useful for the search query:

| Grade | Meaning |
|---:|---|
| 0 | Wrong, unavailable, unsafe under the policy, or unusable |
| 1 | Tangential or weak match |
| 2 | Useful result or plausible variant |
| 3 | Exact intended entity, template, quote, or clearly best match |

For each query, pool the deduplicated top 10 from every system in the release comparison plus manual gold items and hard negatives. Blind and randomize candidates before annotation.

Two annotators grade independently. Their mean is final unless they differ by more than one grade or disagree on safety, in which case a third annotator assigns the final relevance grade and safety class. Separately freeze binary exact-target item or equivalence-group IDs for entity/template and quote/OCR queries; ExactMRR uses those IDs rather than an averaged grade. Report agreement. If a hidden evaluation steward adds unseen candidates, create a judgement-set version and rescore all compared systems.

## Retrieval metrics

### Ranked relevance

Use macro-slice mean **nDCG@10** as the primary quality metric:

```text
gain(relevance) = 2^relevance - 1
DCG@10 = sum(gain(result_i) / log2(i + 1))
nDCG@10 = DCG@10 / ideal_DCG@10
Q = mean(mean(nDCG@10 within each answerable primary slice))
```

An empty or failed response to an answerable query scores `0`. No-match queries are excluded because their ideal DCG is zero.

### Exact findability

Entity/template and quote/OCR benchmark queries each include at least one independently adjudicated exact-target item or equivalence group. Measure how early the first appears:

```text
ExactRR_q = 1 / rank of first exact-target item/group, or 0 when absent from top 10
E = mean(ExactRR_q over entity/template and quote/OCR queries)
```

Also report Hit@1, Hit@5, Hit@10, Recall@10, and MRR by slice for diagnosis.

### Coverage and empty results

```text
Hit@10 =
  answerable queries with at least one relevance >= 2 result in top 10
  --------------------------------------------------------------------
  all answerable queries

CorrectEmpty =
  no-match queries returning an empty result list
  ------------------------------------------------
  all no-match queries

C = 2 * Hit@10 * CorrectEmpty / (Hit@10 + CorrectEmpty)
```

All values are proportions in `[0, 1]`; define `C = 0` when its denominator is zero. This penalizes both always returning a nearest vector and always returning nothing.

Every no-match query requires a versioned corpus-wide audit establishing that no serving item is grade 2 or better. Prefer known-absent entity, template, and quote constructions that can be checked against all canonical fields, then have annotators inspect the corpus for semantic alternatives. Any corpus change invalidates the no-match labels until this audit is repeated.

### Safety and duplicate diagnostics

A displayed list is safe only if every item satisfies the frozen safety policy. Compute safe-list rate separately for ordinary and safety/adversarial subsets, then macro-average them as `S`. An empty list exposes no unsafe item and is safe, but Hit@10 and the answerable safety-slice gate prevent abstention from winning.

Report, but do not score:

- exact duplicate rate in the top 10;
- repeated template-group rate;
- results with missing preview or attribution;
- lexical-only, dense-only, and hybrid contribution by slice.

Diversity is diagnostic rather than rewarded: a focused `john cena` query should not lose points merely because its best results share a subject.

## Performance metrics

Measure both the Python engine and the complete TUI path:

| Metric | Boundary |
|---|---|
| Worker cold start | Process start through compatible `ready` handshake |
| Engine p50/p95/p99 | Worker receives search through serialized result |
| Submit-to-render p50/p95/p99 | User submits through completed TUI result render |
| Error/timeout rate | Failed search requests divided by measured requests |
| Throughput | Completed engine searches per second at declared concurrency |
| Memory | Loaded steady and peak resident memory |
| Storage | SQLite, vectors, thumbnails, and total corpus bytes |

The scored run uses a warm loaded process, concurrency `1`, the frozen corpus/model, result limit `10`, one declared Crossterm-compatible local PTY at `80x24`, 100 warm-ups, and at least 1,000 measured searches. Repeat every query equally in shuffled blocks with result caching disabled. Submit-to-render timing starts when the TUI accepts the submit key and stops only after the backend draws the status and complete returned list. A timeout is charged its full limit and counted as an error. Image-file opening is excluded because it measures the external viewer, not search.

An error is not an empty result. It receives zero relevance, exact-findability, coverage, and safety credit for that request.

## MamboMeme Technical Score — Search v1

**MMTS-Search-v1** is a 0–100 score for the interactive search system. Pipeline throughput is reported separately.

The inputs are:

| Symbol | Component | Weight |
|---|---|---:|
| `Q` | Macro-slice nDCG@10 | 55% |
| `E` | ExactMRR@10 | 15% |
| `C` | Harmonic mean of Hit@10 and CorrectEmpty | 10% |
| `S` | Macro safe-list rate | 10% |
| `L` | `clip((1000 - p95_submit_to_render_ms) / 800)` | 5% |
| `R` | `clip((0.01 - error_rate) / 0.01)` | 5% |

where `clip(x) = min(1, max(0, x))`.

```text
MMTS-Search-v1 = 100 * (
    0.55 * Q
  + 0.15 * E
  + 0.10 * C
  + 0.10 * S
  + 0.05 * L
  + 0.05 * R
)
```

The latency component receives full credit at 200 ms or faster and zero at 1,000 ms. Reliability receives full credit at zero errors and zero at 1%. These are provisional benchmark-v1 targets, not universal hardware claims; changing them creates a score version.

## Hard gates

- A critical safety, rights, privacy, or artifact-integrity violation produces a score of `0` and status `FAIL`.
- Otherwise cap the score at `59` and mark it `FAIL` if any condition is true:
  - a required primary slice has nDCG@10 below `0.30`;
  - ExactMRR@10 is below `0.60`;
  - answerable Hit@10 is below `0.70`;
  - CorrectEmpty is below `0.60`;
  - answerable safety-slice Hit@10 is below `0.70`;
  - safe-list rate is below `0.98`;
  - submit-to-render p95 is at least `1,000 ms`;
  - request error/timeout rate is at least `1%`;
  - the release protocol, publication rollback, or terminal-restoration tests fail.

Interpret a valid score as:

| Score | Meaning |
|---:|---|
| 90–100 | Excellent on this benchmark version |
| 75–89 | Strong |
| 60–74 | Experimental |
| Below 60 | Failed a gate or materially weak |

Always publish components and gates next to the aggregate. Scores compare systems only when benchmark, corpus, policy, hardware, interface, and score versions match.

Hard gates use hidden-test point estimates for a deterministic official result. Also publish a prompt-family bootstrap 95% interval for the score and the fraction of resamples passing every gate.

## Interaction metrics

Opt-in local interaction events can show how people use the ranked list. Only `choose` counts as a successful selection in v1:

- choose rate within the top 10;
- reciprocal chosen rank and chosen-rank distribution;
- time from render to explicit selection;
- preview/open/copy/choose rates;
- reformulation and abandonment rates;
- outcomes by query slice, corpus, retriever, and interface version.

These metrics are not part of MMTS-Search-v1. Preview, open, and copy remain diagnostic actions. Rank and preview position influence behavior, unchosen results may never have been examined, and popular memes receive more familiar interactions. Never train directly on raw counts.

Later improvement can use reviewed selections as examples or run randomized/interleaved comparisons with logged exposure probabilities. Any learned ranker must still improve the frozen human-labelled benchmark without gate regressions.

## Semantic-search acceptance

Semantic retrieval is justified as an experiment, not by the project name. Compare frozen BM25, dense, and hybrid systems on the same hidden benchmark.

Keep the dense route in the default system only if hybrid search improves over BM25 by either:

- at least `+0.03` absolute overall nDCG@10; or
- at least `+0.05` nDCG@10 on the semantic-concept slice;

with a paired-bootstrap 95% confidence interval whose lower bound is above zero, no hard-gate regression, and no entity/template or quote/OCR nDCG regression greater than `0.02`. Otherwise keep the negative experiment report and ship BM25 alone.

## Reproducible report

Freeze and publish:

- corpus manifest and content checksums;
- query, judgement, template-group, and safety-policy versions;
- code commit and complete configuration;
- model identifiers, immutable revisions, preprocessing, and embedding manifest;
- lexical weights, route weights, candidate depth, fusion constant, and empty-result threshold;
- protocol and interface versions;
- hardware and runtime versions;
- fixed random seeds where applicable.

The scorecard contains one row for BM25, dense, and hybrid, followed by any later visual route or reranker. Use development data for every choice; use a fresh hidden version for each exposed release comparison.
