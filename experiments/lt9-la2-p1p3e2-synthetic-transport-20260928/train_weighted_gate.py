"""Fit one weighted logistic cascade and score the internal seed-bank TEST split.

Usage: python train_weighted_gate.py BANK_DIR NEW_OUTPUT_DIR
No relation ID, qrels, retrieval rank, or corpus ID is a model feature.
"""

from __future__ import annotations

import gzip
import hashlib
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

from lexical_gate import FEATURE_NAMES, build_idf, context, decide, features, score


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def read_gzip(path: Path):
    with gzip.open(path, "rt", encoding="utf-8") as stream:
        for line in stream:
            yield json.loads(line)


def fit_logistic(x: np.ndarray, y: np.ndarray, quality: np.ndarray,
                 iterations: int = 240) -> dict:
    # Each class has equal total training mass; provenance quality remains
    # relative within the class, so weak CoInCo negatives do not masquerade
    # as explicit SWORDS rejections.
    sample_weight = quality.astype(np.float64, copy=True)
    for label in (0, 1):
        mask = y == label
        sample_weight[mask] *= 0.5 / max(1e-12, sample_weight[mask].sum())
    sample_weight *= len(y)
    coeff = np.zeros(x.shape[1], dtype=np.float64)
    bias = 0.0
    m = np.zeros_like(coeff)
    v = np.zeros_like(coeff)
    mb = vb = 0.0
    for step in range(1, iterations + 1):
        logits = np.clip(x @ coeff + bias, -40.0, 40.0)
        predictions = 1.0 / (1.0 + np.exp(-logits))
        error = (predictions - y) * sample_weight
        grad = (x.T @ error) / len(y) + 0.005 * coeff
        grad_b = error.mean()
        m = 0.9 * m + 0.1 * grad
        v = 0.999 * v + 0.001 * (grad * grad)
        mb = 0.9 * mb + 0.1 * grad_b
        vb = 0.999 * vb + 0.001 * grad_b * grad_b
        lr = 0.035
        coeff -= lr * (m / (1 - 0.9 ** step)) / (
            np.sqrt(v / (1 - 0.999 ** step)) + 1e-8
        )
        bias -= lr * (mb / (1 - 0.9 ** step)) / (
            np.sqrt(vb / (1 - 0.999 ** step)) + 1e-8
        )
    return {"weights": coeff.tolist(), "bias": bias}


def probability(x: np.ndarray, layer: dict) -> np.ndarray:
    logits = np.clip(x @ np.asarray(layer["weights"]) + layer["bias"], -40, 40)
    return 1.0 / (1.0 + np.exp(-logits))


def choose_sufficiency(dev: list[dict], scores: np.ndarray) -> float:
    # The first engineering pass overfit this threshold to 52 seed-ambiguous
    # DEV rows and rejected every natural E2 packet. A literal absence check
    # in Gate 1 handles erased contexts; the learned sufficiency score remains
    # a 0.5 diagnostic for nonempty evidence. Safety is selected at Gate 2.
    del dev, scores
    return 0.5


def choose_allow(dev: list[dict], score_sufficient: np.ndarray,
                 score_same: np.ndarray, sufficient_threshold: float,
                 x_raw: np.ndarray) -> float:
    labels = np.array([row["label"] for row in dev])
    strengths = np.array([row["label_strength"] for row in dev])
    strong_negative = np.array([
        row["label"] == "DIFFERENT" and (
            "SWORDS_FALSE_VOTE" in row["left_evidence"] or
            "SWORDS_FALSE_VOTE" in row["right_evidence"]
        ) for row in dev
    ])
    negatives = labels == "DIFFERENT"
    positives = labels == "SAME"
    unknown = labels == "UNKNOWN"
    eligible = (score_sufficient >= sufficient_threshold) & (x_raw[:, 2] > 0) & (x_raw[:, 4] > 0)
    best = (-1.0, 1.000001)
    for threshold in np.linspace(0.5, 0.9999, 1000):
        allowed = eligible & (score_same >= threshold)
        if strong_negative.any() and allowed[strong_negative].mean() > 0.01:
            continue
        weighted_error = strengths[allowed & negatives].sum() / max(1e-12, strengths[negatives].sum())
        if weighted_error > 0.01:
            continue
        if unknown.any() and allowed[unknown].mean() > 0.001:
            continue
        coverage = allowed[positives].mean()
        if coverage > best[0]:
            best = (coverage, float(threshold))
    return best[1]


def summarize(rows: list[dict], predictions: list[str]) -> dict:
    summary: dict[str, dict] = {}
    for label in ("SAME", "DIFFERENT", "UNKNOWN"):
        indices = [i for i, row in enumerate(rows) if row["label"] == label]
        counts = Counter(predictions[i] for i in indices)
        summary[label] = {"n": len(indices), **{key: counts[key] for key in
                                      ("ALLOW", "REFUSE", "ABSTAIN")}}
    for origin in ("SYNTHETIC_NO_LOCAL_EVIDENCE", "SEED_UNSURE_OR_MIXED"):
        indices = [i for i, row in enumerate(rows) if row["label_origin"] == origin]
        counts = Counter(predictions[i] for i in indices)
        summary[origin] = {"n": len(indices), **{key: counts[key] for key in
                                                ("ALLOW", "REFUSE", "ABSTAIN")}}
    strong = [i for i, row in enumerate(rows) if row["label"] == "DIFFERENT"
              and ("SWORDS_FALSE_VOTE" in row["left_evidence"] or
                   "SWORDS_FALSE_VOTE" in row["right_evidence"])]
    summary["EXPLICIT_SWORDS_REJECTION"] = {
        "n": len(strong), "false_allow": sum(predictions[i] == "ALLOW" for i in strong)
    }
    return summary


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: train_weighted_gate.py BANK_DIR NEW_OUTPUT_DIR")
    bank, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=False)
    contexts = {row["context_occurrence_id"]: row for row in read_gzip(bank / "contexts.jsonl.gz")}
    pairs = list(read_gzip(bank / "transport-pairs.jsonl.gz"))
    if len(pairs) != 151736:
        raise ValueError(f"unexpected bank length: {len(pairs)}")
    train_context_ids = {value for row in pairs if row["split"] == "train"
                         for value in (row["left_context_id"], row["right_context_id"])}
    idf = build_idf([context(contexts[key]["masked_context"],
                             contexts[key]["source_lemma"], "") for key in sorted(train_context_ids)])
    cache = {}
    arrays = defaultdict(list)
    grouped = defaultdict(list)
    for i, row in enumerate(pairs):
        source, target = row["source_lemma"], row["target_lemma"]
        def get_context(side: str):
            if row["context_transform"] == "BOTH_ENDPOINTS_ERASED":
                return context("[FOCAL]", source, target)
            key = (row[f"{side}_context_id"], source, target)
            if key not in cache:
                cache[key] = context(contexts[key[0]]["masked_context"], source, target)
            return cache[key]
        vector = features(get_context("left"), get_context("right"), idf)
        arrays[row["split"]].append(vector)
        grouped[row["split"]].append(row)
        if (i + 1) % 30000 == 0:
            print(f"features {i + 1}/{len(pairs)}", flush=True)
    train_raw = np.asarray(arrays["train"], dtype=np.float64)
    mean = train_raw.mean(axis=0)
    scale = train_raw.std(axis=0)
    scale[scale < 1e-9] = 1.0
    x = {split: (np.asarray(arrays[split], dtype=np.float64) - mean) / scale
         for split in ("train", "dev", "test")}
    train = grouped["train"]
    suff_y = np.asarray([row["label"] != "UNKNOWN" for row in train], dtype=np.float64)
    suff_quality = np.asarray([0.5 if row["label_origin"] == "SEED_UNSURE_OR_MIXED"
                               else 1.0 for row in train], dtype=np.float64)
    suff = fit_logistic(x["train"], suff_y, suff_quality)
    definite = np.flatnonzero(suff_y)
    compat_y = np.asarray([train[i]["label"] == "SAME" for i in definite], dtype=np.float64)
    compat_quality = np.asarray([train[i]["label_strength"] for i in definite], dtype=np.float64)
    compat = fit_logistic(x["train"][definite], compat_y, compat_quality)
    dev = grouped["dev"]
    dev_suff = probability(x["dev"], suff)
    dev_same = probability(x["dev"], compat)
    t_suff = choose_sufficiency(dev, dev_suff)
    t_allow = choose_allow(dev, dev_suff, dev_same, t_suff,
                           np.asarray(arrays["dev"], dtype=np.float64))
    model = {
        "schema": "phoenix.lexical.weighted-gate/v1",
        "feature_names": FEATURE_NAMES,
        "mean": mean.tolist(), "scale": scale.tolist(),
        "idf": idf, "sufficiency": suff, "compatibility": compat,
        "sufficiency_threshold": t_suff, "allow_threshold": t_allow,
        "refuse_threshold": 0.2, "require_content_anchor": True,
        "training_note": "single weighted logistic cascade; seed-bank proxy labels; no relation ID feature",
    }
    model_path = out / "weighted-gate.json"
    model_path.write_text(json.dumps(model, sort_keys=True, separators=(",", ":")), encoding="utf-8")
    result = {}
    for split in ("dev", "test"):
        rows = grouped[split]
        preds = [decide(model, tuple(vector)) for vector in arrays[split]]
        result[split] = summarize(rows, preds)
        if split == "test":
            with gzip.open(out / "test-decisions.jsonl.gz", "wt", encoding="utf-8") as stream:
                for row, decision in zip(rows, preds):
                    stream.write(json.dumps({"pair_id": row["pair_id"], "label": row["label"],
                                             "label_origin": row["label_origin"],
                                             "decision": decision}, sort_keys=True) + "\n")
    receipt = {
        "schema": "phoenix.lexical.weighted-gate-training-receipt/v1",
        "status": "INTERNAL_PROXY_TEST_COMPLETE_NOT_EXTERNAL_QUALIFICATION",
        "input_pairs_sha256": sha256(bank / "transport-pairs.jsonl.gz"),
        "input_contexts_sha256": sha256(bank / "contexts.jsonl.gz"),
        "model_sha256": sha256(model_path),
        "split_counts": {key: len(grouped[key]) for key in ("train", "dev", "test")},
        "thresholds": {"sufficiency": t_suff, "allow": t_allow, "refuse": 0.2},
        "metrics": result,
        "limitations": ["SWORDS/CoInCo seed semantics are a proxy for transport compatibility",
                        "CoInCo not-elicited negatives are downweighted, not treated as explicit rejections",
                        "internal TEST uses context-disjoint seed-bank split, not independent external labels"],
    }
    (out / "training-receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    print(json.dumps({"thresholds": receipt["thresholds"], "metrics": result}, indent=2))


if __name__ == "__main__":
    main()
