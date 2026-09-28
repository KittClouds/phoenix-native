"""Build a synthetic, insufficient-context UNKNOWN bank without making review packets.

Only visible context packets and their private FIT/HOLDOUT partition fields are
read. Human judgment files, qrels, ranks, retrieval outcomes, and holdout packet
contents are never consumed by this builder.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any


SCHEMA = "phoenix.lexical.p1p3e2.unknown-bank/v1"
TARGET = "UNKNOWN"
TARGET_SEMANTICS = "INSUFFICIENT_OBSERVABLE_LOCAL_CONTEXT"
LABEL_ORIGIN = "SYNTHETIC_BY_CONTEXT_ERASURE"
REDACTED = "[CONTEXT_REDACTED]"
TOKEN_RE = re.compile(r"\w+", re.UNICODE)
VARIANTS = (
    "QUERY_SIDE_ABSENT",
    "DOCUMENT_SIDE_ABSENT",
    "BOTH_SIDES_ABSENT",
    "BOTH_SIDES_REDACTED",
)


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def canonical_json(value: Any) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def read_fit_bases(source_key: str, packet_path: Path, ledger_path: Path) -> list[dict[str, Any]]:
    packets = read_json(packet_path)
    ledger = read_json(ledger_path)
    if not isinstance(packets, list) or not isinstance(ledger, list):
        raise ValueError(f"{source_key}: packets and ledger must be JSON arrays")

    ledger_by_id: dict[str, dict[str, Any]] = {}
    for row in ledger:
        packet_id = row.get("packet_id")
        if not isinstance(packet_id, str) or packet_id in ledger_by_id:
            raise ValueError(f"{source_key}: missing or duplicate ledger packet_id")
        ledger_by_id[packet_id] = row

    seen_packet_ids: set[str] = set()
    bases: list[dict[str, Any]] = []
    for packet in packets:
        packet_id = packet.get("packet_id")
        if not isinstance(packet_id, str) or packet_id in seen_packet_ids:
            raise ValueError(f"{source_key}: missing or duplicate packet_id")
        seen_packet_ids.add(packet_id)
        partition_row = ledger_by_id.get(packet_id)
        if partition_row is None:
            raise ValueError(f"{source_key}: packet has no matching partition row")
        # Deliberately inspect only the frozen partition field from the ledger.
        if partition_row.get("partition") != "FIT":
            continue

        relation = packet.get("lexical_relation")
        query_contexts = packet.get("query_contexts")
        document_contexts = packet.get("document_contexts")
        orientation = packet.get("orientation")
        if (
            not isinstance(relation, list)
            or len(relation) != 2
            or not all(isinstance(term, str) and term for term in relation)
            or not isinstance(query_contexts, list)
            or not isinstance(document_contexts, list)
            or not all(isinstance(value, str) for value in query_contexts + document_contexts)
            or not isinstance(orientation, str)
        ):
            raise ValueError(f"{source_key}: malformed visible context packet {packet_id}")
        if not query_contexts or not document_contexts:
            raise ValueError(f"{source_key}: expected both natural context sides for {packet_id}")
        relation_tokens = {term.casefold() for term in relation}
        for context in query_contexts + document_contexts:
            if relation_tokens.intersection(token.casefold() for token in TOKEN_RE.findall(context)):
                raise ValueError(f"{source_key}: candidate token leaked into context for {packet_id}")

        base_payload = {
            "lexical_relation": relation,
            "orientation": orientation,
            "query_contexts": query_contexts,
            "document_contexts": document_contexts,
        }
        bases.append(
            {
                **base_payload,
                "source_packet_id": packet_id,
                "source_key": source_key,
                "base_content_sha256": sha256_bytes(canonical_json(base_payload)),
            }
        )

    extra_ledger_ids = set(ledger_by_id) - seen_packet_ids
    if extra_ledger_ids:
        raise ValueError(f"{source_key}: unmatched ledger rows: {len(extra_ledger_ids)}")
    return bases


def stable_id(prefix: str, *parts: str) -> str:
    digest = hashlib.sha256("\0".join(parts).encode("utf-8")).hexdigest()[:20]
    return f"{prefix}-{digest}"


def relation_id(relation: list[str]) -> str:
    normalized = [term.casefold() for term in relation]
    return stable_id("rel", normalized[0], normalized[1])


def make_examples(bases: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], dict[str, int]]:
    # Deduplicate exact relation/context payloads so duplicate source rows do not
    # multiply the synthetic UNKNOWN class. Keep every opaque source reference.
    grouped: dict[str, dict[str, Any]] = {}
    for base in bases:
        base_id = stable_id("base", base["base_content_sha256"])
        item = grouped.setdefault(
            base_id,
            {
                "base_id": base_id,
                "base_content_sha256": base["base_content_sha256"],
                "lexical_relation": base["lexical_relation"],
                "orientation": base["orientation"],
                "query_contexts": base["query_contexts"],
                "document_contexts": base["document_contexts"],
                "source_refs": [],
            },
        )
        item["source_refs"].append(
            {"source_key": base["source_key"], "packet_id": base["source_packet_id"]}
        )

    examples: list[dict[str, Any]] = []
    for base_id in sorted(grouped):
        base = grouped[base_id]
        query = base["query_contexts"]
        document = base["document_contexts"]
        variants = {
            "QUERY_SIDE_ABSENT": ([], document),
            "DOCUMENT_SIDE_ABSENT": (query, []),
            "BOTH_SIDES_ABSENT": ([], []),
            "BOTH_SIDES_REDACTED": ([REDACTED], [REDACTED]),
        }
        relation_group = relation_id(base["lexical_relation"])
        for variant in VARIANTS:
            q_contexts, d_contexts = variants[variant]
            example_id = stable_id("unk", base_id, variant)
            examples.append(
                {
                    "schema": SCHEMA,
                    "example_id": example_id,
                    "target": TARGET,
                    "target_semantics": TARGET_SEMANTICS,
                    "label_origin": LABEL_ORIGIN,
                    "variant": variant,
                    "model_input": {
                        "query_contexts": q_contexts,
                        "document_contexts": d_contexts,
                    },
                    "metadata": {
                        "base_group_id": base_id,
                        "relation_group_id": relation_group,
                        "source_orientation": base["orientation"],
                        "source_refs": sorted(
                            base["source_refs"],
                            key=lambda item: (item["source_key"], item["packet_id"]),
                        ),
                        "base_content_sha256": base["base_content_sha256"],
                        "feature_firewall": "Only model_input is feature data; metadata is never a model feature.",
                    },
                }
            )
    examples.sort(key=lambda row: row["example_id"])
    variant_counts = {variant: 0 for variant in VARIANTS}
    for row in examples:
        variant_counts[row["variant"]] += 1
    return examples, {"unique_natural_fit_bases": len(grouped), **variant_counts}


def write_bank(output_dir: Path, examples: list[dict[str, Any]], receipt: dict[str, Any]) -> None:
    output_dir.mkdir(parents=True, exist_ok=False)
    bank_path = output_dir / "synthetic-unknown-bank.jsonl"
    bank_bytes = b"".join(canonical_json(row) + b"\n" for row in examples)
    bank_path.write_bytes(bank_bytes)
    receipt["bank"] = {
        "path": bank_path.name,
        "rows": len(examples),
        "sha256": sha256_bytes(bank_bytes),
        "bytes": len(bank_bytes),
    }
    (output_dir / "build-receipt.json").write_text(
        json.dumps(receipt, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", action="append", nargs=3, metavar=("KEY", "PACKETS", "LEDGER"), required=True)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument(
        "--verify-existing",
        action="store_true",
        help="recompute and compare an existing bank without changing it",
    )
    args = parser.parse_args()
    if args.output_dir.exists() and not args.verify_existing:
        raise SystemExit(f"refusing existing output directory: {args.output_dir}")
    if args.verify_existing and not args.output_dir.is_dir():
        raise SystemExit(f"verification output directory does not exist: {args.output_dir}")

    all_bases: list[dict[str, Any]] = []
    inputs: list[dict[str, Any]] = []
    seen_source_keys: set[str] = set()
    for source_key, packet_arg, ledger_arg in args.source:
        if source_key in seen_source_keys:
            raise SystemExit(f"duplicate source key: {source_key}")
        seen_source_keys.add(source_key)
        packet_path, ledger_path = Path(packet_arg), Path(ledger_arg)
        if not packet_path.is_file() or not ledger_path.is_file():
            raise SystemExit(f"missing source packet or ledger for {source_key}")
        bases = read_fit_bases(source_key, packet_path, ledger_path)
        all_bases.extend(bases)
        inputs.append(
            {
                "source_key": source_key,
                "packet_file_sha256": sha256_file(packet_path),
                "ledger_file_sha256": sha256_file(ledger_path),
                "packet_rows": len(read_json(packet_path)),
                "fit_bases": len(bases),
                "non_fit_rows_excluded": len(read_json(packet_path)) - len(bases),
            }
        )

    examples, counts = make_examples(all_bases)
    receipt: dict[str, Any] = {
        "schema": SCHEMA,
        "status": "SYNTHETIC_ENGINEERING_ABSTENTION_BANK_ONLY",
        "label_semantics": TARGET_SEMANTICS,
        "label_origin": LABEL_ORIGIN,
        "not_claimed": [
            "not human-reviewed natural UNKNOWN evidence",
            "not a semantic claim that the underlying lexical relation is ambiguous",
            "not a compatibility-model result or authority update",
        ],
        "inputs": inputs,
        "source_fit_rows": len(all_bases),
        "source_non_fit_rows_excluded": sum(row["non_fit_rows_excluded"] for row in inputs),
        "deduplicated_base_rows": counts["unique_natural_fit_bases"],
        "deduplicated_source_rows": len(all_bases) - counts["unique_natural_fit_bases"],
        "variant_counts": {key: counts[key] for key in VARIANTS},
        "example_count": len(examples),
        "grouping_rule": "All derived variants from one base_group_id must remain in one future split.",
        "feature_rule": "Use model_input only; relation/source references in metadata are grouping/provenance only.",
        "forbidden_inputs": [
            "human judgment files",
            "qrels labels or grades",
            "retrieval ranks or opportunity outcomes",
            "HOLDOUT context packets",
        ],
    }
    expected_bank = b"".join(canonical_json(row) + b"\n" for row in examples)
    if args.verify_existing:
        bank_path = args.output_dir / "synthetic-unknown-bank.jsonl"
        receipt_path = args.output_dir / "build-receipt.json"
        if not bank_path.is_file() or not receipt_path.is_file():
            raise SystemExit("verification directory is missing generated bank files")
        actual_receipt = read_json(receipt_path)
        expected_hash = sha256_bytes(expected_bank)
        if bank_path.read_bytes() != expected_bank:
            raise SystemExit("existing UNKNOWN bank differs from deterministic rebuild")
        if actual_receipt.get("bank", {}).get("sha256") != expected_hash:
            raise SystemExit("existing receipt does not bind the deterministic bank hash")
        if actual_receipt.get("example_count") != len(examples):
            raise SystemExit("existing receipt has the wrong example count")
        print(f"verified deterministic bank rows={len(examples)} sha256={expected_hash}")
        return 0
    write_bank(args.output_dir, examples, receipt)
    print(f"wrote {args.output_dir / 'synthetic-unknown-bank.jsonl'}")
    print(f"natural FIT bases={counts['unique_natural_fit_bases']} rows={len(examples)}")
    for variant in VARIANTS:
        print(f"{variant}={counts[variant]}")
    print(f"sha256={sha256_bytes((args.output_dir / 'synthetic-unknown-bank.jsonl').read_bytes())}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
