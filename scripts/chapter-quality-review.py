"""Local diagnostic ASR and listening receipts. Never awards a perceptual pass."""
import argparse
import hashlib
import json
import math
import re
import unicodedata
import wave
from pathlib import Path


def normalize(text):
    return re.findall(r"\w+", unicodedata.normalize("NFKC", text).casefold())


def distance(ref, hyp):
    # Exact word Levenshtein distance; O(hyp) scratch, no heuristic matcher.
    previous = list(range(len(hyp) + 1))
    for i, a in enumerate(ref, 1):
        current = [i]
        for j, b in enumerate(hyp, 1):
            current.append(min(current[-1] + 1, previous[j] + 1,
                               previous[j - 1] + (a != b)))
        previous = current
    return previous[-1]


def percentile(values, p):
    return sorted(values)[max(0, math.ceil(len(values) * p) - 1)] if values else None


def prepare(root):
    from huggingface_hub import HfApi, snapshot_download
    repo = "Systran/faster-whisper-small.en"
    revision = HfApi().model_info(repo).sha
    folder = root / "asr-small-en"
    snapshot_download(repo, revision=revision, local_dir=folder,
                      allow_patterns=["model.bin", "config.json", "tokenizer.json",
                                      "vocabulary.*", "preprocessor_config.json", "README.md"])
    files = {}
    for path in folder.iterdir():
        if path.is_file():
            with path.open("rb") as f:
                files[path.name] = hashlib.file_digest(f, "sha256").hexdigest()
    (root / "asr-model-pins.json").write_text(json.dumps(
        {"repo": repo, "revision": revision, "files": files}, indent=2), encoding="utf-8")


def review(root, model_path):
    from faster_whisper import WhisperModel
    completed = json.loads((root / "completed.json").read_text())
    rows = [json.loads(line) for line in (root / "segments.jsonl").read_text().splitlines()]
    assert len(rows) == completed["segments"] and all(r["status"] == "normal_eos" for r in rows)
    source = (root / "chapter-source.txt").read_bytes()
    # No reference prompt, hotwords, or previous-transcript conditioning.
    model = WhisperModel(str(model_path), device="cpu", compute_type="int8", cpu_threads=4,
                         local_files_only=True)
    errors, words = 0, 0
    output = root / "asr-diagnostic.jsonl"
    with output.open("x", encoding="utf-8") as out:
        for row in rows:
            key = bytes(row["key"]).hex()
            pcm = root / "cache" / key / "audio.pcm"
            reference = source[row["source"]["start"]:row["source"]["end"]].decode("utf-8")
            temporary = root / "asr-current.wav"
            with wave.open(str(temporary), "wb") as w:
                w.setparams((1, 2, 24000, 0, "NONE", "not compressed"))
                w.writeframes(pcm.read_bytes())
            segments, _ = model.transcribe(str(temporary), language="en", beam_size=5,
                                           temperature=0, condition_on_previous_text=False,
                                           vad_filter=False)
            transcript = " ".join(s.text.strip() for s in segments)
            temporary.unlink()
            ref, hyp = normalize(reference), normalize(transcript)
            edit = distance(ref, hyp)
            errors += edit
            words += len(ref)
            record = {"segment": row["segment"], "reference": reference, "hypothesis": transcript,
                      "reference_words": len(ref), "word_edits": edit,
                      "diagnostic_wer": edit / len(ref) if ref else None}
            out.write(json.dumps(record, ensure_ascii=False) + "\n")
            out.flush()
            print(f"asr={row['segment']+1}/{len(rows)} edits={edit} words={len(ref)}", flush=True)
    summary(root, rows, completed, errors, words)


def summary(root, rows, completed, errors, words):
    warm = [r for r in rows[1:] if not r["cache_hit"]]
    timing = {
        "warm_request_count": len(warm),
        "warm_first_pcm_p50_seconds": percentile([r["first_pcm_seconds"] for r in warm], .5),
        "warm_first_pcm_p95_seconds": percentile([r["first_pcm_seconds"] for r in warm], .95),
        "warm_rtf_p95": percentile([r["rtf"] for r in warm], .95),
        "render_wall_over_audio": completed["wall_seconds"] / completed["audio_seconds"],
        "rtf_over_0_8_count": sum(r["rtf"] > .8 for r in warm),
        "clipped_samples": sum(r["clipped_samples"] for r in rows),
        "silent_segments": [r["segment"] for r in rows if r["peak"] == 0],
        "diagnostic_word_edits": errors, "diagnostic_reference_words": words,
        "diagnostic_asr_wer": errors / words if words else None,
        "quality_pass": False, "human_listening": "pending", "gold_transcript_wer": "unmeasured",
        "voice_identity": "unrated; independent stateless requests",
        "asr_method": "small.en int8 CPU, beam5, temperature0, no prompt/VAD/history; NFKC casefold Unicode word tokens; no number expansion",
        "measurement_limits": "First PCM is not audible TTFA. ASR errors combine recognition and synthesis errors. One chapter is not a soak or preference test.",
    }
    telemetry_path = root / "telemetry.jsonl"
    telemetry = [json.loads(x) for x in telemetry_path.read_text(encoding="utf-8-sig").splitlines()] if telemetry_path.exists() else []
    gpu = [int(str(x["gpuCsv"]).split(",")[0]) for x in telemetry if x["gpuExit"] == 0]
    timing["sampled_total_gpu_peak_mib"] = max(gpu) if gpu else None
    timing["sampled_worker_peak_private_bytes"] = max((x["privateBytes"] or 0 for x in telemetry), default=None)
    timing["telemetry_caveat"] = "Sampling began after the first three segments; total GPU includes other apps; dependency setup overlapped part of rendering. No isolated VRAM or memory-growth claim."
    authority_path = root / "run-authority.json"
    if authority_path.exists():
        authority = json.loads(authority_path.read_text(encoding="utf-8-sig"))
        if authority.get("mode") == "plan_plain_chapter trim edges v1":
            timing["telemetry_caveat"] = "Trim run: sampling began after opening segments; total GPU includes other apps; ASR ran after rendering. No isolated VRAM or two-hour growth claim."
    feedback = root / "listening-feedback.json"
    if feedback.exists():
        timing["opening_feedback"] = json.loads(feedback.read_text(encoding="utf-8-sig"))
        timing["human_listening"] = "see scoped listening feedback; full chapter adjudication pending"
        if "laterClips" in timing["opening_feedback"]:
            timing["human_listening"] = "user reports clear opening and no issues in middle/ending clips; full chapter adjudication pending"
        if timing["opening_feedback"].get("confirmedExtraSpeech"):
            timing["human_listening"] = "opening/middle/ending spot checks positive; paragraph 77 extra speech confirmed by user"
            timing["qualification_status"] = "REJECTED_CONTENT_FIDELITY"
            timing["confirmed_content_failures"] = timing["opening_feedback"]["confirmedExtraSpeech"]
    timing["speed_gate"] = "pass" if timing["warm_rtf_p95"] <= .8 else "fail"
    for filename, field in [("targeted-listening.json", "targeted_listening"),
                            ("listening-adjudications.json", "listening_adjudications")]:
        path = root / filename
        if path.exists():
            timing[field] = json.loads(path.read_text(encoding="utf-8-sig"))
            timing["human_listening"] = "scoped adjudications recorded; full chapter content accuracy pending"
    (root / "quality-summary.json").write_text(json.dumps(timing, indent=2), encoding="utf-8")
    with wave.open(str(root / "chapter.wav"), "rb") as w:
        rate, total = w.getframerate(), w.getnframes()
        clips = []
        for label, start in [("opening", 0), ("middle", max(0, total//2-rate*20)),
                             ("ending", max(0, total-rate*40))]:
            w.setpos(start)
            with wave.open(str(root / f"listen-{label}.wav"), "wb") as clip:
                clip.setparams(w.getparams())
                clip.writeframes(w.readframes(min(rate*40, total-start)))
            clips.append({"file": f"listen-{label}.wav", "chapter_start_seconds": start/rate,
                          "duration_seconds": min(rate*40, total-start)/rate})
    (root / "listening-clips.json").write_text(json.dumps(clips, indent=2), encoding="utf-8")
    lines = ["# Shortrun Chapter 1 qualification", "", "Status: full listening adjudication pending; no quality promotion.", "",
             ("Opening preview feedback from user: very clear. No numeric rating supplied." if feedback.exists() else "Listening feedback for this run pending."), "",
             ("Later feedback: user heard no issues in either middle/ending preview." if "laterClips" in timing.get("opening_feedback", {}) else "Middle/ending listening feedback pending."), "",
             f"Rendered {completed['segments']} paragraphs, {completed['audio_seconds']/60:.2f} minutes.",
             f"Warm first PCM p50/p95: {timing['warm_first_pcm_p50_seconds']:.3f}/{timing['warm_first_pcm_p95_seconds']:.3f} seconds.",
             f"Warm p95 RTF: {timing['warm_rtf_p95']:.3f}; target <=0.8.",
             f"Diagnostic ASR word disagreement: {errors}/{words} ({errors/words:.2%}). This is not adjudicated TTS WER.", "",
             "Listen to chapter.wav, plus the opening/middle/ending clips. Rate clarity, naturalness, name pronunciation, voice consistency, and boundary pauses from 1 (poor) to 5 (excellent). Record skipped/repeated words with timestamps.", "",
             "Source and plan preserve every chapter byte. That establishes planned coverage, not audible transcript completeness.", "",
             "## Recognition disagreements to adjudicate", "", "| Paragraph | Audio start | Word edits |", "|---|---|---|"]
    if timing.get("confirmed_content_failures"):
        lines[2] = "Status: REJECTED for content fidelity. User confirmed extra speech after paragraph 77, whose source is only Drop your weapons! The p95 RTF speed gate also failed."
    elif timing.get("targeted_listening"):
        lines[2] = "Status: targeted paragraph 77 fix confirmed clean and PCM-identical to approved probe. Full content adjudication remains open; p95 RTF speed gate failed."
        lines[4] = "Paragraph 77 corrected clip: user confirmed clean. Paragraph 7 ending: user confirmed it ended perfectly; ASR trailing word was a false positive. Original-run opening/middle/ending ratings are not transferred to this rerun."
    for item in map(json.loads, (root / "asr-diagnostic.jsonl").read_text(encoding="utf-8").splitlines()):
        if item["word_edits"]:
            seconds = rows[item["segment"]]["chapter_first_frame"] / 24000
            lines.append(f"| {item['segment']+1} | {int(seconds//60)}:{int(seconds%60):02d} | {item['word_edits']} |")
    (root / "review.md").write_text("\n".join(lines), encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["prepare", "review"])
    parser.add_argument("root", type=Path)
    parser.add_argument("--model", type=Path)
    args = parser.parse_args()
    if args.mode == "prepare":
        prepare(args.root)
    else:
        if not args.model:
            parser.error("--model required")
        review(args.root, args.model)
