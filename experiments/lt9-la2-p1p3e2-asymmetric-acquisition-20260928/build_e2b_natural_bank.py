#!/usr/bin/env python3
"""Build the frozen P1P3E2-B natural acquisition bank without labels or qrels."""

from __future__ import annotations

import argparse
import hashlib
import heapq
import json
import re
import sys
import unicodedata
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Iterator


US = "\x1f"
TOKEN_RE = re.compile(r"[A-Za-z0-9]+(?:['’][A-Za-z0-9]+)*")
TERM_PATTERNS = {
    term: re.compile(rf"(?<![A-Za-z0-9]){re.escape(term)}(?![A-Za-z0-9])", re.IGNORECASE)
    for term in ("bank", "lender", "water", "car", "vehicle", "credit", "loan", "engine", "motor", "insurance", "coverage", "debt", "stock", "bond")
}
STOP = frozenset("a an and are as at be been being by for from had has have he her hers him his i if in into is it its of on or our she than that the their them they this to was we were which with you your not but can could do does did may might must should would will shall about after all also any because before between both each few further here how more most other out over same some such through under up very when where who why what without".split())
RELATIONS = [
    ("bank", "lender"), ("bank", "water"), ("car", "vehicle"),
    ("credit", "loan"), ("engine", "motor"), ("insurance", "coverage"),
    ("loan", "debt"), ("stock", "bond"), ("vehicle", "car"),
]
LANES = ("SEMANTIC_NEAR", "SENSE_CONTRAST", "SPARSE_OR_BOUNDARY")
PRIMARY_LANE_SPLITS = {
    "SEMANTIC_NEAR": {"TRAIN-NEW": 5, "DEV-NEW": 1, "TEST-NEW": 2},
    "SENSE_CONTRAST": {"TRAIN-NEW": 5, "DEV-NEW": 2, "TEST-NEW": 1},
    "SPARSE_OR_BOUNDARY": {"TRAIN-NEW": 4, "DEV-NEW": 2, "TEST-NEW": 2},
}


@dataclass(frozen=True, slots=True)
class Occurrence:
    dataset: str
    identity: str
    context: str
    tokens: frozenset[str]
    visible_tokens: int
    source_field: str


@dataclass(frozen=True, slots=True)
class Candidate:
    relation: str
    source: str
    target: str
    dataset: str
    query_id: str
    document_id: str
    query_context: str
    document_context: str
    query_tokens: frozenset[str]
    document_tokens: frozenset[str]
    query_visible: int
    document_visible: int
    query_field: str
    document_field: str
    jaccard: float
    shared: int
    base_group_id: str
    pair_identity: str


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digest_key(seed: int, *parts: str) -> str:
    payload = US.join((str(seed), *parts)).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def nfc(value: str) -> str:
    return unicodedata.normalize("NFC", value)


def canonical_identity(dataset: str, kind: str, identity: str) -> tuple[str, str, str]:
    return dataset.casefold(), kind, identity


def masked_context(text: str, term: str, source: str, target: str, radius: int) -> tuple[str, int] | None:
    """Return one bounded occurrence window and mask both candidate forms."""
    text = nfc(text or "")
    match = TERM_PATTERNS[term].search(text)
    if match is None:
        return None
    tokens = list(TOKEN_RE.finditer(text))
    center = next((i for i, tok in enumerate(tokens) if tok.start() <= match.start() < tok.end()), None)
    if center is None:
        return None
    lo = max(0, center - radius)
    hi = min(len(tokens), center + radius + 1)
    if hi <= lo:
        return None
    chunk = text[tokens[lo].start():tokens[hi - 1].end()]
    # Mask longer forms first to avoid partial overlap if a future roster adds phrases.
    for form, marker in sorted(((source, "[SOURCE]"), (target, "[TARGET]")), key=lambda x: (-len(x[0]), x[0])):
        chunk = TERM_PATTERNS[form].sub(marker, chunk)
    noncandidate = [m.group(0).casefold() for m in TOKEN_RE.finditer(chunk)
                    if m.group(0).casefold() not in (source.casefold(), target.casefold(), "source", "target")]
    return chunk.strip(), len(noncandidate)


def content_tokens(context: str) -> frozenset[str]:
    return frozenset(t.casefold() for t in TOKEN_RE.findall(context)
                     if t.casefold() not in STOP and t.casefold() not in {"source", "target"})


def iter_verified_jsonl(path: Path, expected_bytes: int, expected_sha: str) -> Iterator[dict[str, Any]]:
    h = hashlib.sha256()
    total = 0
    with path.open("rb") as stream:
        for raw in stream:
            h.update(raw)
            total += len(raw)
            if not raw.strip():
                continue
            obj = json.loads(raw)
            if not isinstance(obj, dict):
                raise ValueError(f"non-object JSONL row in {path}")
            yield obj
    actual = h.hexdigest()
    if total != expected_bytes or actual != expected_sha:
        raise ValueError(f"locked source changed: {path} bytes={total} sha256={actual}")


def load_and_validate_lock(path: Path) -> dict[str, Any]:
    lock = json.loads(path.read_text(encoding="utf-8"))
    if lock.get("schema") != "phoenix.lt9.la2.p1p3e2b-acquisition-lock.v1":
        raise ValueError("unexpected acquisition lock schema")
    if lock.get("status") != "FROZEN_BEFORE_CANDIDATE_MINING":
        raise ValueError("lock is not frozen before candidate mining")
    locked_relations = [(row["source"], row["target"]) for row in lock.get("relations", [])]
    if locked_relations != RELATIONS:
        raise ValueError("builder relation roster differs from frozen lock")
    if [row["name"] for row in lock.get("lanes", [])] != list(LANES):
        raise ValueError("builder lane roster differs from frozen lock")
    spec_path = path.parent / lock["protocol_file"]["path"]
    if spec_path.stat().st_size != lock["protocol_file"]["bytes"] or sha256_bytes(spec_path.read_bytes()) != lock["protocol_file"]["sha256"]:
        raise ValueError("acquisition spec does not match lock")
    rubric = path.parent / lock["rubric_file"]["path"]
    if rubric.stat().st_size != lock["rubric_file"]["bytes"] or sha256_bytes(rubric.read_bytes()) != lock["rubric_file"]["sha256"]:
        raise ValueError("rubric does not match lock")
    builder_meta = lock.get("builder_file")
    if not builder_meta:
        raise ValueError("lock does not bind the acquisition builder")
    builder = path.parent / builder_meta["path"]
    if builder.stat().st_size != builder_meta["bytes"] or sha256_bytes(builder.read_bytes()) != builder_meta["sha256"]:
        raise ValueError("acquisition builder does not match lock")
    for source in lock.get("excluded_sources", []):
        source_path = Path(source["path"])
        raw_source = source_path.read_bytes()
        if len(raw_source) != source["bytes"] or sha256_bytes(raw_source) != source["sha256"]:
            raise ValueError(f"locked exclusion source changed: {source_path}")
    excluded = lock["excluded_identity_projection"]
    excluded_path = Path(excluded["path"])
    raw = excluded_path.read_bytes()
    if len(raw) != excluded["bytes"] or sha256_bytes(raw) != excluded["sha256"]:
        raise ValueError("excluded identity projection does not match lock")
    identities = json.loads(raw)
    lock["_excluded"] = {(row["dataset"].casefold(), row["kind"], str(row["id"])) for row in identities}
    return lock


def heap_sample_push(heap: list[tuple[int, str, Occurrence]], capacity: int, rank: str, row: Occurrence) -> None:
    rank_int = int(rank, 16)
    item = (-rank_int, row.identity, row)
    if len(heap) < capacity:
        heapq.heappush(heap, item)
    elif item > heap[0]:
        heapq.heapreplace(heap, item)


def collect_occurrences(lock: dict[str, Any]) -> tuple[dict[tuple[str, str], list[Occurrence]], dict[tuple[str, str], list[Occurrence]], dict[str, int]]:
    seed = lock["seed"]
    excluded = lock["_excluded"]
    sources_by_query: dict[tuple[str, str], list[Occurrence]] = defaultdict(list)
    source_terms = sorted({s for s, _ in RELATIONS})
    target_terms = sorted({t for _, t in RELATIONS})
    query_pattern = re.compile(r"(?<![A-Za-z0-9])(" + "|".join(map(re.escape, source_terms)) + r")(?![A-Za-z0-9])", re.IGNORECASE)
    document_pattern = re.compile(r"(?<![A-Za-z0-9])(" + "|".join(map(re.escape, target_terms)) + r")(?![A-Za-z0-9])", re.IGNORECASE)
    target_heaps: dict[tuple[str, str], list[tuple[int, str, Occurrence]]] = defaultdict(list)
    source_max = {"query_occurrences": 512, "document_occurrences": 2048}
    counts: Counter[str] = Counter()

    for dataset in lock["source_cohort"]:
        name = dataset["name"]
        qmeta = dataset["queries"]
        for row in iter_verified_jsonl(Path(qmeta["path"]), qmeta["bytes"], qmeta["sha256"]):
            counts[f"{name}.query_rows"] += 1
            identity = str(row.get("_id", ""))
            text = str(row.get("text", ""))
            if not identity or not text:
                continue
            if canonical_identity(name, "query", identity) in excluded:
                counts[f"{name}.query_excluded"] += 1
                continue
            found = {m.group(1).casefold() for m in query_pattern.finditer(text)}
            for source in found:
                relation_names = [f"{s}->{t}" for s, t in RELATIONS if s == source]
                for relation in relation_names:
                    target = relation.split("->", 1)[1]
                    result = masked_context(text, source, source, target, 18)
                    if result is None:
                        continue
                    context, visible = result
                    occurrence = Occurrence(name, identity, context, content_tokens(context), visible, "query")
                    key = (name, relation)
                    # query_pool shares a source term across relations; retain per-relation pools for exact caps.
                    sources_by_query[key].append(occurrence)
                    counts[f"{name}.{relation}.query_hits"] += 1
        print(f"verified query source {name}: {counts[f'{name}.query_rows']} rows", file=sys.stderr, flush=True)

        dmeta = dataset["corpus"]
        for row in iter_verified_jsonl(Path(dmeta["path"]), dmeta["bytes"], dmeta["sha256"]):
            counts[f"{name}.document_rows"] += 1
            identity = str(row.get("_id", ""))
            title = str(row.get("title", ""))
            body = str(row.get("text", ""))
            if not identity or not (title or body):
                continue
            if canonical_identity(name, "document", identity) in excluded:
                counts[f"{name}.document_excluded"] += 1
                continue
            combined = (title + " . " + body) if title else body
            hits: dict[str, re.Match[str]] = {}
            for match in document_pattern.finditer(combined):
                hits.setdefault(match.group(1).casefold(), match)
            for target, occurrence_match in hits.items():
                relation_names = [f"{s}->{t}" for s, t in RELATIONS if t == target]
                for relation in relation_names:
                    source = relation.split("->", 1)[0]
                    # Use exact earliest target occurrence; masked_context independently finds that occurrence.
                    result = masked_context(combined, target, source, target, 18)
                    if result is None:
                        continue
                    context, visible = result
                    occurrence = Occurrence(name, identity, context, content_tokens(context), visible, "title+text")
                    key = (name, relation)
                    rank = digest_key(seed, "DOC", name, relation, identity)
                    heap_sample_push(target_heaps[key], source_max["document_occurrences"], rank, occurrence)
                    counts[f"{name}.{relation}.document_hits"] += 1
        print(f"verified corpus source {name}: {counts[f'{name}.document_rows']} rows", file=sys.stderr, flush=True)

    # Convert the bounded deterministic max-heaps to ascending digest order.
    docs: dict[tuple[str, str], list[Occurrence]] = {}
    for key, heap in target_heaps.items():
        docs[key] = [item[2] for item in sorted(heap, key=lambda item: (-item[0], item[1]))]
    queries: dict[tuple[str, str], list[Occurrence]] = {}
    for key, rows in sources_by_query.items():
        relation = key[1]
        dataset = key[0]
        chosen = sorted(rows, key=lambda row: (digest_key(seed, "QUERY", dataset, relation, row.identity), row.identity))[:source_max["query_occurrences"]]
        queries[key] = chosen
    return queries, docs, dict(counts)


def make_candidate(source: str, target: str, q: Occurrence, d: Occurrence, seed: int) -> Candidate:
    relation = f"{source}->{target}"
    union = q.tokens | d.tokens
    shared = len(q.tokens & d.tokens)
    jaccard = (shared / len(union)) if union else 0.0
    pair_identity = f"{q.dataset}\x1f{q.identity}\x1f{d.identity}"
    group_input = US.join((relation, nfc(q.context), nfc(d.context))).encode("utf-8")
    group = "bg-" + sha256_bytes(group_input)[:20]
    return Candidate(relation, source, target, q.dataset, q.identity, d.identity,
                     q.context, d.context, q.tokens, d.tokens, q.visible_tokens,
                     d.visible_tokens, q.source_field, d.source_field, jaccard,
                     shared, group, pair_identity)


def candidate_sort_key(candidate: Candidate, lane: str, seed: int) -> tuple[Any, ...]:
    tie = digest_key(seed, "PAIR", candidate.dataset, candidate.relation,
                     candidate.query_id, candidate.document_id)
    if lane == "SEMANTIC_NEAR":
        return (-candidate.jaccard, -candidate.shared, tie, candidate.pair_identity)
    if lane == "SENSE_CONTRAST":
        return (candidate.jaccard, candidate.shared, tie, candidate.pair_identity)
    return (min(candidate.query_visible, candidate.document_visible),
            -abs(candidate.query_visible - candidate.document_visible),
            candidate.query_visible + candidate.document_visible, tie,
            candidate.pair_identity)


def split_candidates(lock: dict[str, Any], queries: dict[tuple[str, str], list[Occurrence]], docs: dict[tuple[str, str], list[Occurrence]]) -> tuple[list[dict[str, Any]], list[dict[str, Any]], dict[str, Any]]:
    seed = lock["seed"]
    all_by_relation: dict[str, list[Candidate]] = defaultdict(list)
    generation_counts: Counter[str] = Counter()
    dataset_names = [d["name"] for d in lock["source_cohort"]]
    # For each query, inspect a deterministic bounded target-document sample. No relevance or ranking data is used.
    for source, target in RELATIONS:
        relation = f"{source}->{target}"
        for dataset in dataset_names:
            qrows = queries.get((dataset, relation), [])
            drows = docs.get((dataset, relation), [])
            if not qrows or not drows:
                continue
            for q in qrows:
                sampled_docs = sorted(drows, key=lambda d: (digest_key(seed, "PAIR", dataset, relation, q.identity, d.identity), d.identity))[:96]
                for d in sampled_docs:
                    c = make_candidate(source, target, q, d, seed)
                    all_by_relation[relation].append(c)
                    generation_counts[f"{relation}.pairs_generated"] += 1

    all_rows: list[dict[str, Any]] = []
    underfill: dict[str, Any] = {}
    global_identity_split: dict[tuple[str, str, str], str] = {}
    for source, target in RELATIONS:
        relation = f"{source}->{target}"
        candidates = all_by_relation.get(relation, [])
        # Collapse exact masked-context duplicates before the split; retain the smallest deterministic source key.
        unique: dict[tuple[str, str], Candidate] = {}
        for c in candidates:
            key = (nfc(c.query_context), nfc(c.document_context))
            existing = unique.get(key)
            rank = digest_key(seed, "DUP", c.dataset, c.query_id, c.document_id)
            if existing is None or rank < digest_key(seed, "DUP", existing.dataset, existing.query_id, existing.document_id):
                unique[key] = c
        lane_pools = {lane: sorted(unique.values(), key=lambda c: candidate_sort_key(c, lane, seed)) for lane in LANES}
        used_context_keys: set[tuple[str, str]] = set()
        identity_split = global_identity_split
        corpus_primary: Counter[str] = Counter()
        corpus_reserve: Counter[str] = Counter()
        counts = {lane: Counter() for lane in LANES}
        for lane in LANES:
            lane_key = PRIMARY_LANE_SPLITS[lane]
            for split_name in ("TRAIN-NEW", "DEV-NEW", "TEST-NEW"):
                target_n = lane_key[split_name]
                for c in lane_pools[lane]:
                    ctxkey = (nfc(c.query_context), nfc(c.document_context))
                    if ctxkey in used_context_keys or corpus_primary[c.dataset] >= 12:
                        continue
                    qkey = canonical_identity(c.dataset, "query", c.query_id)
                    dkey = canonical_identity(c.dataset, "document", c.document_id)
                    if any(identity_split.get(k, split_name) != split_name for k in (qkey, dkey)):
                        continue
                    if counts[lane][split_name] >= target_n:
                        break
                    identity_split[qkey] = split_name
                    identity_split[dkey] = split_name
                    used_context_keys.add(ctxkey)
                    corpus_primary[c.dataset] += 1
                    counts[lane][split_name] += 1
                    row = candidate_row(c, lane, split_name, seed, primary=True)
                    all_rows.append(row)
                    if counts[lane][split_name] >= target_n:
                        break
        # Mechanical reserve queue: four per lane, TRAIN-only; exclude all contexts selected primary.
        reserve_counts: Counter[str] = Counter()
        for lane in LANES:
            for c in lane_pools[lane]:
                if reserve_counts[lane] >= 4:
                    break
                ctxkey = (nfc(c.query_context), nfc(c.document_context))
                if ctxkey in used_context_keys or corpus_reserve[c.dataset] >= 6:
                    continue
                qkey = canonical_identity(c.dataset, "query", c.query_id)
                dkey = canonical_identity(c.dataset, "document", c.document_id)
                if any(identity_split.get(k, "TRAIN-NEW") != "TRAIN-NEW" for k in (qkey, dkey)):
                    continue
                identity_split[qkey] = "TRAIN-NEW"
                identity_split[dkey] = "TRAIN-NEW"
                used_context_keys.add(ctxkey)
                corpus_reserve[c.dataset] += 1
                reserve_counts[lane] += 1
                all_rows.append(candidate_row(c, lane, "RESERVE-TRAIN", seed, primary=False))
        for lane in LANES:
            missing = {split: need - counts[lane][split] for split, need in PRIMARY_LANE_SPLITS[lane].items() if counts[lane][split] < need}
            if missing or reserve_counts[lane] < 4:
                underfill[f"{relation}.{lane}"] = {'primary': dict(counts[lane]), 'primary_missing': missing, 'reserve': reserve_counts[lane], 'reserve_missing': max(0, 4 - reserve_counts[lane]), 'unique_pair_candidates': len(unique)}
    # Generate blind packet IDs, then deterministic randomized display order.
    primary = [row for row in all_rows if row["population"] != "RESERVE"]
    for row in all_rows:
        row["packet_id"] = "e2b-" + digest_key(seed, "PACKET", row["base_group_id"])[:16]
    primary.sort(key=lambda row: (digest_key(seed, "DISPLAY", row["packet_id"]), row["packet_id"]))
    all_rows.sort(key=lambda row: (row["relation"], row["lane"], row["population"], row["packet_id"]))
    report = {'generated_pair_counts':dict(generation_counts),'underfill':underfill,'primary_count':len(primary),'reserve_count':len(all_rows)-len(primary)}
    return all_rows, primary, report


def candidate_row(c: Candidate, lane: str, split: str, seed: int, primary: bool) -> dict[str, Any]:
    return {
        'packet_id':'', 'base_group_id':c.base_group_id, 'relation':c.relation,
        'source':c.source, 'target':c.target, 'dataset':c.dataset,
        'query_id':c.query_id, 'document_id':c.document_id,
        'lane':lane, 'split':split if primary else 'TRAIN-NEW',
        'population':'PRIMARY' if primary else 'RESERVE',
        'query_context':c.query_context, 'document_context':c.document_context,
        'query_non_candidate_tokens':c.query_visible,
        'document_non_candidate_tokens':c.document_visible,
        'content_jaccard':round(c.jaccard,8), 'shared_content_tokens':c.shared,
        'query_field_source':c.query_field, 'document_field_source':c.document_field,
        'label':None
    }


def create_artifacts(lock_path: Path, out: Path) -> dict[str, Any]:
    lock = load_and_validate_lock(lock_path)
    expected_outputs = ("private-candidate-ledger.jsonl", "review-packets.json", "judgments-template.json", "acquisition-receipt.json")
    if any((out / name).exists() for name in expected_outputs):
        raise FileExistsError(f"refusing to overwrite an existing acquisition artifact under {out}")
    queries, docs, read_counts = collect_occurrences(lock)
    rows, primary, selection_report = split_candidates(lock, queries, docs)
    out.mkdir(parents=True, exist_ok=True)
    private_path = out / "private-candidate-ledger.jsonl"
    with private_path.open("w", encoding="utf-8", newline="\n") as stream:
        for row in rows:
            stream.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")
    review_packets = [
        {'packet_id':row['packet_id'],'lexical_relation':row['relation'],
         'query_context':row['query_context'],'document_context':row['document_context']}
        for row in primary
    ]
    review_path = out / "review-packets.json"
    review_path.write_text(json.dumps(review_packets, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    template = [{'packet_id':row['packet_id'],'judgment':None} for row in primary]
    template_path = out / "judgments-template.json"
    template_path.write_text(json.dumps(template, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    primary_counts: Counter[str] = Counter()
    reserve_counts: Counter[str] = Counter()
    corpora: dict[str, Counter[str]] = defaultdict(Counter)
    split_identity_sets: dict[str, set[tuple[str, str, str]]] = defaultdict(set)
    for row in rows:
        if row['population'] == 'PRIMARY':
            primary_counts[f"{row['relation']}|{row['lane']}|{row['split']}"] += 1
        else:
            reserve_counts[f"{row['relation']}|{row['lane']}"] += 1
        corpora[row['relation']][row['dataset']] += 1
        if row['population'] == 'PRIMARY':
            split_identity_sets[row['split']].add(canonical_identity(row['dataset'],'query',row['query_id']))
            split_identity_sets[row['split']].add(canonical_identity(row['dataset'],'document',row['document_id']))
    overlaps={}
    splits=('TRAIN-NEW','DEV-NEW','TEST-NEW')
    for i,a in enumerate(splits):
        for b in splits[i+1:]:
            overlaps[f"{a}∩{b}"]=len(split_identity_sets[a]&split_identity_sets[b])
    manifest={
        'schema':'phoenix.lt9.la2.p1p3e2b-acquisition-receipt.v1',
        'status':'ACQUISITION_COMPLETE_LABELS_UNOPENED',
        'lock_sha256':sha256_bytes(lock_path.read_bytes()),
        'read_counts':read_counts,
        'query_occurrence_pools':{f"{d}|{r}":len(v) for (d,r),v in sorted(queries.items())},
        'document_occurrence_pools':{f"{d}|{r}":len(v) for (d,r),v in sorted(docs.items())},
        'primary_count':len(primary),'reserve_count':len(rows)-len(primary),
        'primary_counts_by_relation_lane_split':dict(sorted(primary_counts.items())),
        'reserve_counts_by_relation_lane':dict(sorted(reserve_counts.items())),
        'source_counts_by_relation_corpus':{r:dict(sorted(c.items())) for r,c in sorted(corpora.items())},
        'identity_overlap_between_primary_splits':overlaps,
        'underfill':selection_report['underfill'],
        'generated_pair_counts':selection_report['generated_pair_counts'],
        'review_packet_fields':['packet_id','lexical_relation','query_context','document_context'],
        'review_packets_randomized':True,'labels_joined':False,'qrels_opened':False,
        'model_or_e1_contact':False,'fitting_or_retrieval_run':False,
    }
    for path in (private_path, review_path, template_path):
        manifest[path.name]={'bytes':path.stat().st_size,'sha256':sha256_bytes(path.read_bytes())}
    receipt_path=out/'acquisition-receipt.json'
    receipt_path.write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    return manifest


def main() -> int:
    parser=argparse.ArgumentParser()
    parser.add_argument('--lock',type=Path,default=Path(__file__).with_name('P1P3E2B_ACQUISITION_LOCK.json'))
    parser.add_argument('--out',type=Path,default=Path(r'D:\phoenix-evals\lt9-la2-p1p3e2b-acquisition-20260928\bank-v1'))
    args=parser.parse_args()
    try:
        manifest=create_artifacts(args.lock,args.out)
    except Exception as exc:
        print(f"P1P3E2B acquisition failed closed: {exc}",file=sys.stderr)
        return 2
    print(json.dumps({'status':manifest['status'],'primary_count':manifest['primary_count'],'reserve_count':manifest['reserve_count'],'underfilled_groups':len(manifest['underfill']),'receipt':str(args.out/'acquisition-receipt.json')},indent=2))
    return 0


if __name__=='__main__':
    raise SystemExit(main())
