"""Read-only target-score anatomy for the four reachable E2 QPS opportunities."""

import json
import sys
from pathlib import Path

import numpy as np
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

from lfm230_readout import encode_texts, probability, serialize


def main():
    if len(sys.argv) != 5:
        raise SystemExit("usage: inspect_lfm_qps.py PRETRAINED READOUT QPS_SEARCHES OUTPUT_JSON")
    pretrained, readout_path, searches_path, output = (Path(value) for value in sys.argv[1:])
    artifact = json.loads(readout_path.read_text(encoding="utf-8"))
    rows = [json.loads(line) for line in searches_path.read_text(encoding="utf-8").splitlines()]
    texts = []
    refs = []
    for row in rows:
        candidate = row["candidate"]
        source, target = candidate["direction"].split("->")
        for hit in row["new_context_hits"]:
            if hit["ordinal"] == candidate["document_ordinal"]:
                texts.append(serialize(source, target, row["query_masked_context"],
                                       hit["masked_context"]))
                refs.append({"dataset": candidate["dataset"],
                             "relation": candidate["direction"],
                             "query_id": candidate["query_id"],
                             "document_id": candidate["document_id"],
                             "target_ordinal": hit["ordinal"]})
    tokenizer = AutoTokenizer.from_pretrained(pretrained)
    tokenizer.pad_token = tokenizer.eos_token
    model = AutoModelForCausalLM.from_pretrained(pretrained, dtype=torch.float16).to("cuda").eval()
    vectors, timing = encode_texts(model, tokenizer, texts)
    x = (vectors - np.asarray(artifact["mean"])) / np.asarray(artifact["scale"])
    enough = probability(x, artifact["sufficiency"])
    same = probability(x, artifact["compatibility"])
    for ref, p_enough, p_same in zip(refs, enough, same):
        ref["sufficiency_score"] = float(p_enough)
        ref["compatibility_score"] = float(p_same)
    output.write_text(json.dumps({"targets": refs, "timing": timing}, indent=2), encoding="utf-8")
    print(json.dumps(refs, indent=2))


if __name__ == "__main__":
    main()
