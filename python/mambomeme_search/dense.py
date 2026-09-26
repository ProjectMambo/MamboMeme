"""Deterministic TF-IDF/LSA artifacts and exact cosine search."""

from __future__ import annotations

import json
import math
import os
from collections import Counter
from pathlib import Path
from typing import Iterable

import numpy as np

from .text import search_tokens


MODEL_VERSION = "tfidf-lsa-v1"
MAX_DIMENSIONS = 8


def _write_json(path: Path, value: object) -> None:
    content = (
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        + "\n"
    ).encode("utf-8")
    with path.open("wb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())


def _write_npy(path: Path, value: np.ndarray) -> None:
    with path.open("wb") as stream:
        np.save(stream, value, allow_pickle=False)
        stream.flush()
        os.fsync(stream.fileno())


def build_lsa_artifacts(
    documents: Iterable[tuple[str, str]], destination: Path
) -> dict[str, object]:
    """Build a small exact dense index without a model download."""

    ordered = sorted(documents)
    ids = [item_id for item_id, _ in ordered]
    tokenized = [search_tokens(document) for _, document in ordered]
    vocabulary = sorted({token for tokens in tokenized for token in tokens})
    token_index = {token: index for index, token in enumerate(vocabulary)}

    counts = np.zeros((len(ids), len(vocabulary)), dtype=np.float64)
    for row, tokens in enumerate(tokenized):
        for token, count in Counter(tokens).items():
            counts[row, token_index[token]] = 1.0 + math.log(count)

    if ids and vocabulary:
        document_frequency = np.count_nonzero(counts, axis=0)
        idf = np.log((1.0 + len(ids)) / (1.0 + document_frequency)) + 1.0
        tfidf = counts * idf
        u, singular, components = np.linalg.svd(tfidf, full_matrices=False)
        dimensions = min(MAX_DIMENSIONS, len(singular))
        for index in range(dimensions):
            pivot = int(np.argmax(np.abs(components[index])))
            if components[index, pivot] < 0:
                components[index] *= -1
                u[:, index] *= -1
        components = components[:dimensions]
        vectors = u[:, :dimensions] * singular[:dimensions]
        norms = np.linalg.norm(vectors, axis=1, keepdims=True)
        vectors = np.divide(vectors, norms, out=np.zeros_like(vectors), where=norms > 0)
    else:
        dimensions = 0
        idf = np.zeros((len(vocabulary),), dtype=np.float64)
        components = np.zeros((0, len(vocabulary)), dtype=np.float64)
        vectors = np.zeros((len(ids), 0), dtype=np.float64)

    _write_json(destination / "dense_ids.json", ids)
    _write_json(destination / "dense_vocab.json", vocabulary)
    _write_npy(destination / "dense_idf.npy", idf.astype("<f4"))
    _write_npy(destination / "dense_components.npy", components.astype("<f4"))
    _write_npy(destination / "dense_vectors.npy", vectors.astype("<f4"))
    return {
        "dense_dimension": dimensions,
        "dense_model": MODEL_VERSION,
        "dense_vocabulary_size": len(vocabulary),
        "numpy_version": np.__version__,
    }


class DenseIndex:
    """Validated in-memory LSA index."""

    def __init__(self, snapshot: Path) -> None:
        self.ids = json.loads((snapshot / "dense_ids.json").read_text("utf-8"))
        vocabulary = json.loads((snapshot / "dense_vocab.json").read_text("utf-8"))
        self.idf = np.load(snapshot / "dense_idf.npy", allow_pickle=False)
        self.components = np.load(snapshot / "dense_components.npy", allow_pickle=False)
        self.vectors = np.load(snapshot / "dense_vectors.npy", allow_pickle=False)
        if (
            not isinstance(self.ids, list)
            or any(not isinstance(item_id, str) for item_id in self.ids)
            or len(set(self.ids)) != len(self.ids)
            or not isinstance(vocabulary, list)
            or any(not isinstance(token, str) for token in vocabulary)
            or vocabulary != sorted(set(vocabulary))
        ):
            raise ValueError("invalid dense ID or vocabulary artifact")
        if self.idf.shape != (len(vocabulary),):
            raise ValueError("dense IDF shape mismatch")
        if self.components.ndim != 2 or self.components.shape[1] != len(vocabulary):
            raise ValueError("dense component shape mismatch")
        if self.vectors.shape != (len(self.ids), self.components.shape[0]):
            raise ValueError("dense vector/ID shape mismatch")
        if not all(
            np.isfinite(value).all()
            for value in (self.idf, self.components, self.vectors)
        ):
            raise ValueError("dense artifacts contain non-finite values")
        norms = np.linalg.norm(self.vectors, axis=1)
        if norms.size and np.any((norms > 1e-6) & (np.abs(norms - 1.0) > 1e-4)):
            raise ValueError("dense vectors are not normalized")
        self.vocabulary = {token: index for index, token in enumerate(vocabulary)}

    def search(self, query: str, minimum_similarity: float = 0.15) -> list[tuple[str, float]]:
        counts = Counter(search_tokens(query))
        vector = np.zeros((len(self.vocabulary),), dtype=np.float32)
        for token, count in counts.items():
            if (index := self.vocabulary.get(token)) is not None:
                vector[index] = (1.0 + math.log(count)) * self.idf[index]
        if not np.any(vector) or self.components.shape[0] == 0:
            return []
        projected = vector @ self.components.T
        norm = float(np.linalg.norm(projected))
        if norm == 0:
            return []
        similarities = self.vectors @ (projected / norm)
        ranked = sorted(
            (
                (self.ids[index], float(score))
                for index, score in enumerate(similarities)
                if score >= minimum_similarity
            ),
            key=lambda item: (-item[1], item[0]),
        )
        return ranked
