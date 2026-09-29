"""Small, deterministic feature surface for the synthetic transport engineering gate.

Candidate strings select the action but never contribute contextual evidence.
The same module scores bank pairs and QPS query/document contexts.
"""

from __future__ import annotations

import math
import re
from collections import Counter
from dataclasses import dataclass

TOKEN_RE = re.compile(r"\[FOCAL\]|\[SOURCE\]|\[TARGET\]|[a-z0-9]+", re.I)
STOP = frozenset(
    "a an and are as at be been but by for from had has have he her hers him his i in into "
    "is it its of on or our she that the their them there these they this those to was we "
    "were what when where which who will with you your".split()
)
NEGATE = frozenset("no not never neither without unlike instead rather".split())
SUPPORT = frozenset("same similar equivalent also like means meant called named".split())
FEATURE_NAMES = (
    "min_tokens", "max_tokens", "min_content", "length_ratio", "shared_content",
    "content_jaccard", "idf_overlap", "shared_role", "shared_near", "shared_bigrams",
    "cross_role", "shared_stop", "negation_agrees", "support_agrees",
    "negation_any", "focal_left_delta", "focal_right_delta", "no_content_overlap",
)


@dataclass(frozen=True, slots=True)
class Context:
    tokens: frozenset[str]
    content: frozenset[str]
    stop: frozenset[str]
    left: frozenset[str]
    right: frozenset[str]
    near: frozenset[str]
    bigrams: frozenset[tuple[str, str]]
    n_tokens: int
    n_content: int
    n_left: int
    n_right: int
    negation: bool
    support: bool


def context(text: str, source: str, target: str) -> Context:
    words = TOKEN_RE.findall(text.lower())
    focal = next((i for i, word in enumerate(words) if word in
                  ("[focal]", "[source]", "[target]")), len(words) // 2)
    blocked = {source.lower(), target.lower()}
    usable = [(i, word) for i, word in enumerate(words)
              if word not in blocked and not word.startswith("[")]
    content_words = [(i, word) for i, word in usable if word not in STOP]
    valid_positions = {i: word for i, word in content_words}
    return Context(
        tokens=frozenset(word for _, word in usable),
        content=frozenset(word for _, word in content_words),
        stop=frozenset(word for _, word in usable if word in STOP),
        left=frozenset(word for i, word in content_words if i < focal),
        right=frozenset(word for i, word in content_words if i > focal),
        near=frozenset(word for i, word in content_words if abs(i - focal) <= 4),
        bigrams=frozenset((word, valid_positions[i + 1]) for i, word in content_words
                           if i + 1 in valid_positions),
        n_tokens=len(usable),
        n_content=len(content_words),
        n_left=sum(i < focal for i, _ in usable),
        n_right=sum(i > focal for i, _ in usable),
        negation=any(word in NEGATE for _, word in usable),
        support=any(word in SUPPORT for _, word in usable),
    )


def build_idf(contexts: list[Context]) -> dict[str, float]:
    df = Counter(word for ctx in contexts for word in ctx.content)
    n = len(contexts)
    return {word: math.log1p((n + 1) / (count + 1)) for word, count in df.items()}


def features(left: Context, right: Context, idf: dict[str, float]) -> tuple[float, ...]:
    shared = left.content & right.content
    union = left.content | right.content
    shared_weight = sum(idf.get(word, 1.0) for word in shared)
    union_weight = sum(idf.get(word, 1.0) for word in union)
    shared_role = len(left.left & right.left) + len(left.right & right.right)
    cross_role = len(left.left & right.right) + len(left.right & right.left)
    return (
        math.log1p(min(left.n_tokens, right.n_tokens)),
        math.log1p(max(left.n_tokens, right.n_tokens)),
        math.log1p(min(left.n_content, right.n_content)),
        min(left.n_tokens, right.n_tokens) / max(1, max(left.n_tokens, right.n_tokens)),
        math.log1p(len(shared)),
        len(shared) / max(1, len(union)),
        shared_weight / max(1.0, union_weight),
        math.log1p(shared_role),
        math.log1p(len(left.near & right.near)),
        math.log1p(len(left.bigrams & right.bigrams)),
        math.log1p(cross_role),
        math.log1p(len(left.stop & right.stop)),
        float(left.negation == right.negation),
        float(left.support == right.support),
        float(left.negation or right.negation),
        math.log1p(abs(left.n_left - right.n_left)),
        math.log1p(abs(left.n_right - right.n_right)),
        float(not shared),
    )


def score(model: dict, vector: tuple[float, ...]) -> tuple[float, float]:
    def predict(layer: dict) -> float:
        z = layer["bias"] + sum(
            weight * (value - mean) / scale
            for value, weight, mean, scale in zip(
                vector, layer["weights"], model["mean"], model["scale"]
            )
        )
        return 1.0 / (1.0 + math.exp(-max(-40.0, min(40.0, z))))

    return predict(model["sufficiency"]), predict(model["compatibility"])


def decide(model: dict, vector: tuple[float, ...]) -> str:
    enough, same = score(model, vector)
    # Gate 1 cannot infer evidence from geometry when either endpoint has no
    # visible content. Synthetic both-absent examples must remain abstentions.
    if vector[2] == 0.0:
        return "ABSTAIN"
    if enough < model["sufficiency_threshold"]:
        return "ABSTAIN"
    if model["require_content_anchor"] and vector[4] == 0.0:
        return "ABSTAIN"
    if same >= model["allow_threshold"]:
        return "ALLOW"
    if same <= model["refuse_threshold"]:
        return "REFUSE"
    return "ABSTAIN"
