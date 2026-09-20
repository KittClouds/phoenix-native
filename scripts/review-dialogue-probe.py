"""Diagnostic comparison of local probe audio, with no reference prompting."""
import json
import sys
import wave
from pathlib import Path
from faster_whisper import WhisperModel

root = Path(sys.argv[1])
model = WhisperModel(sys.argv[2], device="cpu", compute_type="int8", cpu_threads=4,
                     local_files_only=True)
rows = json.loads((root / "cases.json").read_text(encoding="utf-8"))
with (root / "asr.jsonl").open("x", encoding="utf-8") as output:
    for row in rows:
        if row["status"] != "normal_eos":
            continue
        path = root / (row["case"] + ".wav")
        with wave.open(str(path), "wb") as wav:
            wav.setparams((1, 2, 24000, 0, "NONE", "not compressed"))
            wav.writeframes((root / (row["case"] + ".pcm")).read_bytes())
        parts, _ = model.transcribe(str(path), language="en", beam_size=5, temperature=0,
                                    condition_on_previous_text=False, vad_filter=False)
        row["hypothesis"] = " ".join(p.text.strip() for p in parts)
        output.write(json.dumps(row, ensure_ascii=False) + "\n")
        output.flush()
        print(row["case"], row["hypothesis"], flush=True)
