"""Frozen 230M representation + two tiny logistic readouts, engineering comparison.

The seed bank supplies proxy labels. This script samples a bounded, deterministic
subset of its already context-disjoint TRAIN/DEV/TEST partitions, then scores
the existing reviewed E2 packets and full-corpus QPS expansion candidates.
"""

from __future__ import annotations

import gzip
import hashlib
import json
import random
import re
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

from lexical_gate import context, features

SEED = 20260928
MAX_TOKENS = 160
BATCH = 16
CAPS = {
    "train": {"SAME": 1600, "STRONG_DIFFERENT": 800, "WEAK_DIFFERENT": 800,
              "ERASED_UNKNOWN": 1600, "SEED_UNKNOWN": 200},
    "dev": {"SAME": 400, "STRONG_DIFFERENT": 200, "WEAK_DIFFERENT": 400,
            "ERASED_UNKNOWN": 400, "SEED_UNKNOWN": 40},
    "test": {"SAME": 400, "STRONG_DIFFERENT": 200, "WEAK_DIFFERENT": 400,
             "ERASED_UNKNOWN": 400, "SEED_UNKNOWN": 40},
}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def jsonl_gzip(path: Path):
    with gzip.open(path, "rt", encoding="utf-8") as stream:
        for line in stream:
            yield json.loads(line)


def category(row: dict) -> str:
    if row["label"] == "SAME":
        return "SAME"
    if row["label"] == "UNKNOWN":
        return "ERASED_UNKNOWN" if row["context_transform"] == "BOTH_ENDPOINTS_ERASED" else "SEED_UNKNOWN"
    strong = any("SWORDS_FALSE_VOTE" in row[side] for side in ("left_evidence", "right_evidence"))
    return "STRONG_DIFFERENT" if strong else "WEAK_DIFFERENT"


def choose_rows(bank: Path) -> dict[str, list[dict]]:
    buckets = defaultdict(list)
    for row in jsonl_gzip(bank / "transport-pairs.jsonl.gz"):
        buckets[row["split"], category(row)].append(row)
    rng = random.Random(SEED)
    selected = {}
    for split in CAPS:
        values = []
        for class_name, cap in CAPS[split].items():
            rows = sorted(buckets[split, class_name], key=lambda row: row["pair_id"])
            values.extend(rng.sample(rows, min(cap, len(rows))))
        selected[split] = sorted(values, key=lambda row: row["pair_id"])
    return selected


def local(text: str, source: str, target: str) -> str:
    text = text.replace("[SOURCE]", "[FOCAL]").replace("[TARGET]", "[FOCAL]")
    marker = "[FOCAL]"
    at = text.find(marker)
    if at >= 0:
        text = text[max(0, at - 110):at + len(marker) + 110]
    for term in (source, target):
        if term:
            text = re.sub(r"\b" + re.escape(term) + r"\b", "[MASKED]", text,
                          flags=re.IGNORECASE)
    return text


def serialize(source: str, target: str, left: str, right: str) -> str:
    return (f"RELATION: {source} -> {target}\n"
            f"QUERY: {local(left, source, target)}\n"
            f"DOCUMENT: {local(right, source, target)}")


def bank_input(row: dict, contexts: dict[str, dict]) -> str:
    if row["context_transform"] == "BOTH_ENDPOINTS_ERASED":
        left = right = ""
    else:
        left = contexts[row["left_context_id"]]["masked_context"]
        right = contexts[row["right_context_id"]]["masked_context"]
    return serialize(row["source_lemma"], row["target_lemma"], left, right)


def encode_texts(model, tokenizer, texts: list[str]) -> tuple[np.ndarray, dict]:
    vectors = np.empty((len(texts), model.config.hidden_size), dtype=np.float32)
    at_max = 0
    total_seconds = 0.0
    for start in range(0, len(texts), BATCH):
        chunk = texts[start:start + BATCH]
        tokenized = tokenizer(chunk, return_tensors="pt", padding=True,
                              truncation=True, max_length=MAX_TOKENS).to("cuda")
        lengths = tokenized.attention_mask.sum(dim=1) - 1
        at_max += int((lengths + 1 == MAX_TOKENS).sum().item())
        tick = time.perf_counter()
        with torch.inference_mode():
            output = model(**tokenized, output_hidden_states=True, use_cache=False)
            state = output.hidden_states[-1]
            pooled = state[torch.arange(len(chunk), device="cuda"), lengths]
            vectors[start:start + len(chunk)] = pooled.float().cpu().numpy()
        torch.cuda.synchronize()
        total_seconds += time.perf_counter() - tick
        if start and (start // BATCH) % 75 == 0:
            print(f"LFM embeddings {start}/{len(texts)}", flush=True)
    return vectors, {"rows": len(texts), "seconds": total_seconds, "at_max_length": at_max}


def fit_readout(x: np.ndarray, y: np.ndarray, quality: np.ndarray) -> dict:
    weights = quality.copy().astype(np.float32)
    for label in (0, 1):
        mask = y == label
        weights[mask] *= 0.5 / max(1e-12, weights[mask].sum())
    weights *= len(y)
    x_tensor = torch.from_numpy(x).to("cuda")
    y_tensor = torch.from_numpy(y.astype(np.float32)).to("cuda")
    w_tensor = torch.from_numpy(weights).to("cuda")
    readout = torch.nn.Linear(x.shape[1], 1).to("cuda")
    torch.nn.init.zeros_(readout.weight)
    torch.nn.init.zeros_(readout.bias)
    optimizer = torch.optim.AdamW(readout.parameters(), lr=0.01, weight_decay=0.01)
    for _ in range(160):
        optimizer.zero_grad(set_to_none=True)
        logits = readout(x_tensor).squeeze(1)
        loss = (torch.nn.functional.binary_cross_entropy_with_logits(
            logits, y_tensor, reduction="none") * w_tensor).mean()
        loss.backward()
        optimizer.step()
    return {"weights": readout.weight.detach().cpu().numpy().ravel().tolist(),
            "bias": float(readout.bias.detach().cpu().item())}


def probability(x: np.ndarray, layer: dict) -> np.ndarray:
    z = np.clip(x @ np.asarray(layer["weights"]) + layer["bias"], -40, 40)
    return 1 / (1 + np.exp(-z))


def decision(enough: np.ndarray, same: np.ndarray, t_enough: float,
             t_allow: float) -> list[str]:
    return ["ABSTAIN" if e < t_enough else "ALLOW" if s >= t_allow
            else "REFUSE" if s <= 0.2 else "ABSTAIN" for e, s in zip(enough, same)]


def select_thresholds(rows: list[dict], enough: np.ndarray,
                      same: np.ndarray) -> tuple[float, float]:
    labels = np.array([row["label"] for row in rows])
    strong = np.array([category(row) == "STRONG_DIFFERENT" for row in rows])
    different = labels == "DIFFERENT"
    unknown = labels == "UNKNOWN"
    same_rows = labels == "SAME"
    strength = np.asarray([row["label_strength"] for row in rows])
    best = (-1.0, 0.999999, 1.000001)
    for t_enough in np.linspace(0.3, 0.95, 14):
        eligible = enough >= t_enough
        for t_allow in np.linspace(0.5, 0.999, 180):
            allow = eligible & (same >= t_allow)
            if strong.any() and allow[strong].sum() > 0:
                continue
            if unknown.any() and allow[unknown].mean() > 0.01:
                continue
            weak_risk = strength[allow & different].sum() / max(1e-12, strength[different].sum())
            if weak_risk > 0.01:
                continue
            coverage = allow[same_rows].mean()
            if coverage > best[0]:
                best = (float(coverage), float(t_enough), float(t_allow))
    return best[1], best[2]


def metrics(rows: list[dict], predictions: list[str]) -> dict:
    result = {}
    for label in ("SAME", "DIFFERENT", "UNKNOWN"):
        found = [prediction for row, prediction in zip(rows, predictions) if row["label"] == label]
        result[label] = {"n": len(found), **dict(Counter(found))}
    for cat in ("STRONG_DIFFERENT", "WEAK_DIFFERENT", "ERASED_UNKNOWN", "SEED_UNKNOWN"):
        found = [prediction for row, prediction in zip(rows, predictions) if category(row) == cat]
        result[cat] = {"n": len(found), **dict(Counter(found))}
    return result


def main() -> None:
    if len(sys.argv) != 6:
        raise SystemExit("usage: lfm230_readout.py BANK_DIR MODEL_DIR E2_REVIEW_DIR QPS_SEARCHES NEW_OUTPUT_DIR")
    bank, pretrained, review, searches, out = (Path(part) for part in sys.argv[1:])
    out.mkdir(parents=True, exist_ok=False)
    selected = choose_rows(bank)
    contexts = {row["context_occurrence_id"]: row for row in jsonl_gzip(bank / "contexts.jsonl.gz")}
    tokenizer = AutoTokenizer.from_pretrained(pretrained)
    tokenizer.pad_token = tokenizer.eos_token
    model = AutoModelForCausalLM.from_pretrained(pretrained, dtype=torch.float16).to("cuda").eval()
    encoded = {}
    timings = {}
    for split in ("train", "dev", "test"):
        texts = [bank_input(row, contexts) for row in selected[split]]
        encoded[split], timings[split] = encode_texts(model, tokenizer, texts)
        print(split, timings[split], flush=True)
    mean = encoded["train"].mean(axis=0)
    scale = encoded["train"].std(axis=0)
    scale[scale < 1e-6] = 1
    x = {key: ((value - mean) / scale).astype(np.float32) for key, value in encoded.items()}
    train = selected["train"]
    sufficient = np.array([row["label"] != "UNKNOWN" for row in train])
    suff_quality = np.array([0.5 if category(row) == "SEED_UNKNOWN" else 1.0 for row in train])
    suff = fit_readout(x["train"], sufficient.astype(np.float32), suff_quality)
    definite = np.flatnonzero(sufficient)
    compat_y = np.array([train[i]["label"] == "SAME" for i in definite], dtype=np.float32)
    compat_quality = np.array([train[i]["label_strength"] for i in definite])
    compat = fit_readout(x["train"][definite], compat_y, compat_quality)
    dev_enough = probability(x["dev"], suff)
    dev_same = probability(x["dev"], compat)
    t_enough, t_allow = select_thresholds(selected["dev"], dev_enough, dev_same)
    result = {}
    for split in ("dev", "test"):
        preds = decision(probability(x[split], suff), probability(x[split], compat),
                         t_enough, t_allow)
        result[split] = metrics(selected[split], preds)
    artifact = {
        "schema": "phoenix.lexical.lfm230-frozen-readout/v1",
        "model_revision": "9d2be5519834990d30996f878b6771cccbd24f2c",
        "model_safetensors_sha256": sha256(pretrained / "model.safetensors"),
        "tokenizer_sha256": sha256(pretrained / "tokenizer.json"),
        "max_tokens": MAX_TOKENS, "pooling": "final_hidden_state_at_last_nonpadding_token",
        "serialization": "RELATION / QUERY / DOCUMENT, no chat template, candidate forms masked in contexts",
        "mean": mean.tolist(), "scale": scale.tolist(),
        "sufficiency": suff, "compatibility": compat,
        "sufficiency_threshold": t_enough, "allow_threshold": t_allow,
        "refuse_threshold": 0.2,
    }
    (out / "lfm230-readout.json").write_text(json.dumps(artifact, separators=(",", ":")), encoding="utf-8")

    packets = json.loads((review / "review-packets.json").read_text(encoding="utf-8"))
    labels = {row["packet_id"]: row["judgment"] for row in
              json.loads((review / "judgments-user-pass1.json").read_text(encoding="utf-8"))}
    e2_texts = [serialize(*packet["lexical_relation"], packet["query_contexts"][0],
                          packet["document_contexts"][0]) for packet in packets]
    e2_vectors, timings["e2_packets"] = encode_texts(model, tokenizer, e2_texts)
    e2_x = ((e2_vectors - mean) / scale).astype(np.float32)
    e2_pred = decision(probability(e2_x, suff), probability(e2_x, compat), t_enough, t_allow)
    e2_summary = defaultdict(Counter)
    for packet, pred in zip(packets, e2_pred):
        e2_summary[labels[packet["packet_id"]]][pred] += 1

    search_rows = [json.loads(line) for line in searches.read_text(encoding="utf-8").splitlines()]
    qps_items = []
    qps_texts = []
    for row_index, row in enumerate(search_rows):
        source, target = row["candidate"]["direction"].split("->")
        for hit in row["new_context_hits"]:
            qps_items.append((row_index, hit["ordinal"]))
            qps_texts.append(serialize(source, target, row["query_masked_context"], hit["masked_context"]))
    qps_vectors, timings["qps"] = encode_texts(model, tokenizer, qps_texts)
    qps_x = ((qps_vectors - mean) / scale).astype(np.float32)
    qps_pred = decision(probability(qps_x, suff), probability(qps_x, compat), t_enough, t_allow)
    qps_decisions = defaultdict(dict)
    for (row_index, ordinal), pred in zip(qps_items, qps_pred):
        qps_decisions[str(row_index)][str(ordinal)] = pred
    (out / "lfm230-qps-decisions.json").write_text(json.dumps(qps_decisions), encoding="utf-8")
    receipt = {
        "schema": "phoenix.lexical.lfm230-engineering-readout/v1",
        "status": "FROZEN_REPRESENTATION_PROXY_ENGINEERING_NOT_AUTHORITY",
        "sampling_seed": SEED, "selected_counts": {split: dict(Counter(category(row) for row in rows))
                                            for split, rows in selected.items()},
        "selected_pair_id_sha256": {split: hashlib.sha256("\n".join(row["pair_id"] for row in rows).encode()).hexdigest()
                                    for split, rows in selected.items()},
        "thresholds": {"sufficiency": t_enough, "allow": t_allow, "refuse": 0.2},
        "metrics": result,
        "e2_packet_confusion": {label: dict(counts) for label, counts in e2_summary.items()},
        "qps_decision_counts": dict(Counter(qps_pred)),
        "timings": timings,
        "readout_sha256": sha256(out / "lfm230-readout.json"),
        "limits": ["bounded deterministic subset of the seed bank, not all 151736 rows",
                   "seed substitution judgments are proxy labels for transport compatibility",
                   "reviewed E2 packets are discovery-exposed engineering sanity checks"],
    }
    (out / "lfm230-receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    print(json.dumps({"thresholds": receipt["thresholds"], "metrics": result,
                      "e2_packet_confusion": receipt["e2_packet_confusion"],
                      "qps_decision_counts": receipt["qps_decision_counts"]}, indent=2))


if __name__ == "__main__":
    main()
