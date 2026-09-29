"""Exploratory 0.5/0.5 endpoint policy; same frozen readout, no retraining."""

from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

from direct_endpoint_lfm import category, prompt, selected_observations
from lfm230_readout import encode_texts, jsonl_gzip, probability, sha256

T_SUFFICIENT = 0.5
T_ACCEPT = 0.5


def main() -> None:
    if len(sys.argv) != 7:
        raise SystemExit("usage: direct_endpoint_midpoint.py BANK MODEL_DIR READOUT E2_REVIEW QPS_SEARCHES NEW_OUTPUT_DIR")
    bank, pretrained, readout_path, review, searches, out = (Path(x) for x in sys.argv[1:])
    out.mkdir(parents=True, exist_ok=False)
    artifact = json.loads(readout_path.read_text(encoding="utf-8"))
    mean, scale = np.asarray(artifact["mean"]), np.asarray(artifact["scale"])
    tokenizer = AutoTokenizer.from_pretrained(pretrained)
    tokenizer.pad_token = tokenizer.eos_token
    model = AutoModelForCausalLM.from_pretrained(pretrained, dtype=torch.float16).to("cuda").eval()

    contexts = {row["context_occurrence_id"]: row
                for row in jsonl_gzip(bank / "contexts.jsonl.gz")}
    test_rows = selected_observations(bank)["test"]
    test_texts = [prompt(row["source_lemma"], row["target_lemma"],
                         contexts[row["context_occurrence_id"]]["masked_context"])
                  for row in test_rows]
    test_vectors, test_timing = encode_texts(model, tokenizer, test_texts)
    test_x = ((test_vectors - mean) / scale).astype(np.float32)
    test_enough = probability(test_x, artifact["sufficiency"])
    test_accept = probability(test_x, artifact["acceptability"])
    test_by_class = defaultdict(Counter)
    for row, enough, accept in zip(test_rows, test_enough, test_accept):
        verdict = "ALLOW" if enough >= T_SUFFICIENT and accept >= T_ACCEPT else "ABSTAIN"
        test_by_class[category(row)][verdict] += 1

    packets = json.loads((review / "review-packets.json").read_text(encoding="utf-8"))
    labels = {row["packet_id"]: row["judgment"] for row in
              json.loads((review / "judgments-user-pass1.json").read_text(encoding="utf-8"))}
    search_rows = [json.loads(line) for line in searches.read_text(encoding="utf-8").splitlines()]
    texts = []
    refs = []
    for packet in packets:
        source, target = packet["lexical_relation"]
        left = len(texts)
        texts.extend((prompt(source, target, packet["query_contexts"][0]),
                      prompt(target, source, packet["document_contexts"][0])))
        refs.append(("packet", packet["packet_id"], left, left + 1))
    for row_index, row in enumerate(search_rows):
        source, target = row["candidate"]["direction"].split("->")
        for hit in row["new_context_hits"]:
            left = len(texts)
            texts.extend((prompt(source, target, row["query_masked_context"]),
                          prompt(target, source, hit["masked_context"])))
            refs.append(("qps", (row_index, hit["ordinal"]), left, left + 1))
    vectors, app_timing = encode_texts(model, tokenizer, texts)
    x = ((vectors - mean) / scale).astype(np.float32)
    enough = probability(x, artifact["sufficiency"])
    accept = probability(x, artifact["acceptability"])
    endpoint_allows = (enough >= T_SUFFICIENT) & (accept >= T_ACCEPT)
    packet_counts = defaultdict(Counter)
    qps_decisions = defaultdict(dict)
    reachable = []
    for kind, identity, left, right in refs:
        pair_allow = bool(endpoint_allows[left] and endpoint_allows[right])
        if kind == "packet":
            packet_counts[labels[identity]]["ALLOW" if pair_allow else "ABSTAIN"] += 1
        else:
            row_index, ordinal = identity
            qps_decisions[str(row_index)][str(ordinal)] = "ALLOW" if pair_allow else "ABSTAIN"
            row = search_rows[row_index]
            if ordinal == row["candidate"]["document_ordinal"]:
                reachable.append({"relation": row["candidate"]["direction"],
                                  "target_ordinal": ordinal,
                                  "decision": "ALLOW" if pair_allow else "ABSTAIN",
                                  "query_accept": float(accept[left]),
                                  "document_accept": float(accept[right])})
    decision_path = out / "direct-midpoint-qps-decisions.json"
    decision_path.write_text(json.dumps(qps_decisions), encoding="utf-8")
    receipt = {
        "schema": "phoenix.lexical.direct-endpoint-midpoint/v1",
        "status": "EXPLORATORY_MIDPOINT_POLICY_DISCOVERY_EXPOSED_NOT_DEPLOYABLE",
        "readout_sha256": sha256(readout_path),
        "thresholds": {"sufficiency": T_SUFFICIENT, "acceptability": T_ACCEPT},
        "internal_test": {kind: dict(counts) for kind, counts in test_by_class.items()},
        "e2_packet_confusion": {label: dict(counts) for label, counts in packet_counts.items()},
        "qps_decision_counts": dict(Counter(decision for values in qps_decisions.values()
                                             for decision in values.values())),
        "reachable_targets": reachable,
        "timings": {"internal_test": test_timing, "application": app_timing},
        "decision_sha256": sha256(decision_path),
        "limits": ["0.5 midpoint is an engineering diagnostic after seeing prior failures",
                   "reviewed E2 rows are discovery-exposed; no qualification or serving claim"],
    }
    (out / "direct-midpoint-receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    print(json.dumps({"internal_test": receipt["internal_test"],
                      "e2_packet_confusion": receipt["e2_packet_confusion"],
                      "reachable_targets": reachable}, indent=2))


if __name__ == "__main__":
    main()
