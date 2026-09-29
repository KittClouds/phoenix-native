"""Score the 47 reviewed E2 natural packets as an application sanity check."""

import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

from lexical_gate import context, decide, features, score


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: score_e2_packets.py MODEL E2_REVIEW_DIR NEW_OUTPUT_JSON")
    model = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    root, output = Path(sys.argv[2]), Path(sys.argv[3])
    packets = json.loads((root / "review-packets.json").read_text(encoding="utf-8"))
    judgments = json.loads((root / "judgments-user-pass1.json").read_text(encoding="utf-8"))
    labels = {row["packet_id"]: row["judgment"] for row in judgments}
    confusion = defaultdict(Counter)
    details = []
    for packet in packets:
        source, target = packet["lexical_relation"]
        left = context(packet["query_contexts"][0], source, target)
        right = context(packet["document_contexts"][0], source, target)
        vector = features(left, right, model["idf"])
        decision = decide(model, vector)
        label = labels[packet["packet_id"]]
        confusion[label][decision] += 1
        details.append({"packet_id": packet["packet_id"], "relation": f"{source}->{target}",
                        "label": label, "decision": decision, "scores": score(model, vector),
                        "shared_content": vector[4]})
    result = {"schema": "phoenix.lexical.weighted-gate-e2-packets/v1",
              "status": "DISCOVERY_APPLICATION_SANITY_NOT_QUALIFICATION",
              "confusion": {label: dict(counts) for label, counts in confusion.items()},
              "details": details}
    output.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result["confusion"], indent=2))


if __name__ == "__main__":
    main()
