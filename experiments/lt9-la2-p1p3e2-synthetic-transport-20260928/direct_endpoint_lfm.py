"""One focused repair: learn substitution permission at each endpoint.

The pairwise SWORDS/CoInCo join labels were a poor proxy for natural QPS
compatibility. This engineering diagnostic uses the existing direct seed
observation label for source -> target in its own context, then requires
permission at both the query and document endpoint. No new data is acquired.
"""

from __future__ import annotations

import hashlib
import json
import random
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

from lfm230_readout import encode_texts, fit_readout, jsonl_gzip, local, probability, sha256

SEED = 20260928
CAPS = {
    "train": {"SAME": 4000, "STRONG_DIFFERENT": 2000,
              "WEAK_DIFFERENT": 2000, "UNKNOWN": 800},
    "dev": {"SAME": 800, "STRONG_DIFFERENT": 400,
            "WEAK_DIFFERENT": 400, "UNKNOWN": 200},
    "test": {"SAME": 800, "STRONG_DIFFERENT": 400,
             "WEAK_DIFFERENT": 400, "UNKNOWN": 200},
}


def category(row: dict) -> str:
    if row["label"] == "SAME":
        return "SAME"
    if row["label"] == "UNKNOWN":
        return "UNKNOWN"
    return "STRONG_DIFFERENT" if row["evidence_type"] == "SWORDS_FALSE_VOTE" else "WEAK_DIFFERENT"


def selected_observations(bank: Path) -> dict[str, list[dict]]:
    buckets = defaultdict(list)
    for row in jsonl_gzip(bank / "substitution-observations.jsonl.gz"):
        buckets[row["split"], category(row)].append(row)
    rng = random.Random(SEED)
    result = {}
    for split, quotas in CAPS.items():
        chosen = []
        for kind, cap in quotas.items():
            rows = sorted(buckets[split, kind], key=lambda row: row["observation_id"])
            chosen.extend(rng.sample(rows, min(cap, len(rows))))
        result[split] = sorted(chosen, key=lambda row: row["observation_id"])
    return result


def prompt(source: str, target: str, text: str) -> str:
    return f"RELATION: {source} -> {target}\nCONTEXT: {local(text, source, target)}"


def label_weight(row: dict) -> float:
    return float(row["confidence"])


def decisions(enough: np.ndarray, accept: np.ndarray, t_enough: float,
              t_accept: float) -> list[str]:
    return ["ABSTAIN" if e < t_enough else "ALLOW" if a >= t_accept
            else "REFUSE" if a < 0.2 else "ABSTAIN" for e, a in zip(enough, accept)]


def choose_threshold(rows: list[dict], enough: np.ndarray,
                     accept: np.ndarray) -> tuple[float, float, float]:
    labels = np.array([row["label"] for row in rows])
    strong = np.array([category(row) == "STRONG_DIFFERENT" for row in rows])
    weak = np.array([category(row) == "WEAK_DIFFERENT" for row in rows])
    unknown = labels == "UNKNOWN"
    positive = labels == "SAME"
    best = (-1.0, 0.5, 1.000001)
    for te in (0.3, 0.5, 0.7):
        eligible = enough >= te
        for ta in np.linspace(0.5, 0.999, 250):
            allowed = eligible & (accept >= ta)
            if strong.any() and allowed[strong].mean() > 0.02:
                continue
            if weak.any() and allowed[weak].mean() > 0.05:
                continue
            if unknown.any() and allowed[unknown].mean() > 0.02:
                continue
            coverage = allowed[positive].mean()
            if coverage > best[0]:
                best = (float(coverage), float(te), float(ta))
    return best


def metrics(rows: list[dict], predictions: list[str]) -> dict:
    result = {}
    for kind in ("SAME", "STRONG_DIFFERENT", "WEAK_DIFFERENT", "UNKNOWN", "ERASED"):
        values = [decision for row, decision in zip(rows, predictions)
                  if row.get("category") == kind]
        result[kind] = {"n": len(values), **dict(Counter(values))}
    return result


def main() -> None:
    if len(sys.argv) != 6:
        raise SystemExit("usage: direct_endpoint_lfm.py BANK MODEL E2_REVIEW QPS_SEARCHES NEW_OUTPUT_DIR")
    bank, pretrained, review, searches, out = (Path(x) for x in sys.argv[1:])
    out.mkdir(parents=True, exist_ok=False)
    sampled = selected_observations(bank)
    contexts = {row["context_occurrence_id"]: row
                for row in jsonl_gzip(bank / "contexts.jsonl.gz")}
    tokenizer = AutoTokenizer.from_pretrained(pretrained)
    tokenizer.pad_token = tokenizer.eos_token
    model = AutoModelForCausalLM.from_pretrained(pretrained, dtype=torch.float16).to("cuda").eval()
    encoded = {}
    examples = {}
    timings = {}
    for split, rows in sampled.items():
        entries = [dict(row, category=category(row)) for row in rows]
        # A deterministic erased companion teaches Gate 1 the no-evidence case.
        rng = random.Random(SEED + len(rows))
        for row in rng.sample(rows, min(len(rows) // 4, 2000)):
            entries.append(dict(row, category="ERASED", label="UNKNOWN", erased=True))
        texts = [prompt(row["source_lemma"], row["target_lemma"],
                        "" if row.get("erased") else contexts[row["context_occurrence_id"]]["masked_context"])
                 for row in entries]
        encoded[split], timings[split] = encode_texts(model, tokenizer, texts)
        examples[split] = entries
        print(split, timings[split], flush=True)
    mean = encoded["train"].mean(axis=0)
    scale = encoded["train"].std(axis=0)
    scale[scale < 1e-6] = 1
    x = {split: ((values - mean) / scale).astype(np.float32)
         for split, values in encoded.items()}
    train = examples["train"]
    sufficient = np.array([row["label"] != "UNKNOWN" for row in train])
    suff_weight = np.array([0.5 if row["category"] == "UNKNOWN" else 1.0 for row in train])
    suff = fit_readout(x["train"], sufficient.astype(np.float32), suff_weight)
    definite = np.flatnonzero(sufficient)
    accept_y = np.array([train[i]["label"] == "SAME" for i in definite], dtype=np.float32)
    accept_weight = np.array([label_weight(train[i]) for i in definite])
    accept = fit_readout(x["train"][definite], accept_y, accept_weight)
    dev_enough = probability(x["dev"], suff)
    dev_accept = probability(x["dev"], accept)
    coverage, t_enough, t_accept = choose_threshold(examples["dev"], dev_enough, dev_accept)
    scores = {}
    for split in ("dev", "test"):
        predictions = decisions(probability(x[split], suff), probability(x[split], accept),
                                t_enough, t_accept)
        scores[split] = metrics(examples[split], predictions)
    artifact = {
        "schema": "phoenix.lexical.direct-endpoint-lfm230/v1",
        "model_revision": "9d2be5519834990d30996f878b6771cccbd24f2c",
        "model_safetensors_sha256": sha256(pretrained / "model.safetensors"),
        "tokenizer_sha256": sha256(pretrained / "tokenizer.json"),
        "mean": mean.tolist(), "scale": scale.tolist(),
        "sufficiency": suff, "acceptability": accept,
        "sufficiency_threshold": t_enough, "acceptability_threshold": t_accept,
        "dev_same_coverage": coverage,
        "serialization": "RELATION / CONTEXT; both endpoint candidate forms masked in context",
    }
    (out / "direct-endpoint-readout.json").write_text(json.dumps(artifact, separators=(",", ":")), encoding="utf-8")

    packets = json.loads((review / "review-packets.json").read_text(encoding="utf-8"))
    labels = {row["packet_id"]: row["judgment"] for row in
              json.loads((review / "judgments-user-pass1.json").read_text(encoding="utf-8"))}
    searches_rows = [json.loads(line) for line in searches.read_text(encoding="utf-8").splitlines()]
    endpoints = []
    pairs = []
    for packet in packets:
        source, target = packet["lexical_relation"]
        start = len(endpoints)
        endpoints.extend((prompt(source, target, packet["query_contexts"][0]),
                          prompt(target, source, packet["document_contexts"][0])))
        pairs.append(("packet", packet["packet_id"], start, start + 1))
    for row_index, row in enumerate(searches_rows):
        source, target = row["candidate"]["direction"].split("->")
        for hit in row["new_context_hits"]:
            start = len(endpoints)
            endpoints.extend((prompt(source, target, row["query_masked_context"]),
                              prompt(target, source, hit["masked_context"])))
            pairs.append(("qps", (row_index, hit["ordinal"]), start, start + 1))
    extra_vectors, timings["application_endpoints"] = encode_texts(model, tokenizer, endpoints)
    extra_x = ((extra_vectors - mean) / scale).astype(np.float32)
    extra_enough = probability(extra_x, suff)
    extra_accept = probability(extra_x, accept)
    endpoint_predictions = decisions(extra_enough, extra_accept, t_enough, t_accept)
    packet_counts = defaultdict(Counter)
    qps_decisions = defaultdict(dict)
    reachable = []
    for kind, identity, left, right in pairs:
        pair_decision = ("ALLOW" if endpoint_predictions[left] == "ALLOW"
                         and endpoint_predictions[right] == "ALLOW" else "ABSTAIN")
        if kind == "packet":
            packet_counts[labels[identity]][pair_decision] += 1
        else:
            row_index, ordinal = identity
            qps_decisions[str(row_index)][str(ordinal)] = pair_decision
            row = searches_rows[row_index]
            if ordinal == row["candidate"]["document_ordinal"]:
                reachable.append({"relation": row["candidate"]["direction"],
                                  "target_ordinal": ordinal, "decision": pair_decision,
                                  "query_accept": float(extra_accept[left]),
                                  "document_accept": float(extra_accept[right]),
                                  "query_sufficient": float(extra_enough[left]),
                                  "document_sufficient": float(extra_enough[right])})
    (out / "direct-endpoint-qps-decisions.json").write_text(json.dumps(qps_decisions), encoding="utf-8")
    receipt = {
        "schema": "phoenix.lexical.direct-endpoint-lfm230-receipt/v1",
        "status": "FOCUSED_ENGINEERING_REPAIR_DIAGNOSTIC_NOT_AUTHORITY",
        "bank_observations_sha256": sha256(bank / "substitution-observations.jsonl.gz"),
        "selection_seed": SEED,
        "selected_counts": {split: dict(Counter(row["category"] for row in entries))
                            for split, entries in examples.items()},
        "thresholds": {"sufficiency": t_enough, "acceptability": t_accept},
        "dev_same_coverage": coverage, "internal_metrics": scores,
        "e2_packet_confusion": {label: dict(counts) for label, counts in packet_counts.items()},
        "qps_decisions": dict(Counter(decision for values in qps_decisions.values()
                                      for decision in values.values())),
        "reachable_targets": reachable, "timings": timings,
        "readout_sha256": sha256(out / "direct-endpoint-readout.json"),
        "limits": ["same seed corpus, not external evidence", "pair requires both directed endpoint permissions",
                   "E2 labels are discovery-exposed application sanity evidence"],
    }
    (out / "direct-endpoint-receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    print(json.dumps({"thresholds": receipt["thresholds"], "internal_metrics": scores,
                      "e2_packet_confusion": receipt["e2_packet_confusion"],
                      "reachable_targets": reachable}, indent=2))


if __name__ == "__main__":
    main()
