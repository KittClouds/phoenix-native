//! Offline chapter qualification. Exact plain-text paragraph ranges; no UI promotion.
use phoenix_reader_session::{
    AudioCache, ByteRange, Chapter, DocumentBinding, MappingKind, MappingRun, NarrationPlan,
    PlanSpec, Segment,
};
use phoenix_tts_native::{Bundle, Cancellation, NativeProvider, Request};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Seek, SeekFrom, Write},
    path::Path,
    time::{Duration, Instant},
};
fn hash(b: &[u8]) -> [u8; 32] {
    *blake3::hash(b).as_bytes()
}
fn wav_header(frames: u64) -> Vec<u8> {
    let n = u32::try_from(frames * 2).unwrap();
    let mut b = Vec::with_capacity(44);
    b.extend_from_slice(b"RIFF");
    b.extend((n + 36).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(24000u32.to_le_bytes());
    b.extend(48000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend(n.to_le_bytes());
    b
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 6 {
        return Err("usage: chapter_quality WORKER MODEL DLL_DIR SOURCE OUTPUT".into());
    }
    let source = fs::read_to_string(&a[4])?;
    if !source.starts_with("Chapter 1: Quicksave") {
        return Err("unexpected chapter authority".into());
    }
    let end = source.find("\nChapter 2:").ok_or("missing chapter end")? + 1;
    let chapter = &source[..end];
    let root = Path::new(&a[5]);
    fs::create_dir(root)?;
    fs::write(root.join("chapter-source.txt"), chapter)?;
    // Each nonempty line starts a paragraph. Preserve every intervening byte.
    let mut starts = vec![0];
    let mut at = 0;
    for line in chapter.split_inclusive('\n') {
        if at > 0 && !line.trim().is_empty() {
            starts.push(at);
        }
        at += line.len();
    }
    starts.push(chapter.len());
    let segments: Vec<_> = starts
        .windows(2)
        .enumerate()
        .map(|(i, w)| {
            let range = ByteRange {
                start: w[0] as u32,
                end: w[1] as u32,
            };
            Segment {
                chapter: 0,
                sentence: i as u32,
                source: range,
                spoken: range,
            }
        })
        .collect();
    if segments
        .iter()
        .any(|s| s.spoken.slice(chapter).unwrap().len() > 1024)
    {
        return Err("paragraph exceeds frozen budget".into());
    }
    let whole = ByteRange {
        start: 0,
        end: chapter.len() as u32,
    };
    let plan = NarrationPlan::new(
        chapter,
        PlanSpec {
            document: DocumentBinding {
                workspace: hash(a[4].as_bytes()),
                entry: 1,
                revision: 1,
                content: hash(chapter.as_bytes()),
            },
            planner: hash(b"qualification/exact-plain-paragraphs/v1;not-sentence-alignment"),
            pronunciation: hash(b"identity"),
            rules: Box::new([]),
            spoken: chapter.into(),
            mappings: vec![MappingRun {
                source: whole,
                spoken: whole,
                kind: MappingKind::Copy,
                rule: 0,
            }]
            .into(),
            chapters: vec![Chapter { source: whole }].into(),
            segments: segments.into(),
        },
    )?;
    let plan = if std::env::var_os("PHOENIX_QUALIFY_TRIM_EDGES").is_some() {
        phoenix_reader_session::plan_plain_chapter(chapter, plan.spec().document)?
    } else {
        plan
    };
    fs::write(
        root.join("plan.json"),
        serde_json::to_vec_pretty(
            &json!({"source_path":a[4],"full_source_blake3":hash(source.as_bytes()),"chapter_start":0,"chapter_end":end,"plan_id":plan.id(),"spec":plan.spec(),"quality_status":"unrated","wer_status":"not_measured","seed":42,"direction":"A calm, clear English narrator."}),
        )?,
    )?;
    let bundle = Bundle::open(Path::new(&a[1]), Path::new(&a[2]), Path::new(&a[3]))?;
    let mut provider =
        NativeProvider::new(bundle, Duration::from_secs(240), Duration::from_secs(120))?;
    let mut cache = AudioCache::open(root.join("cache"), 256 * 1024 * 1024)?;
    let mut rows = BufWriter::new(File::create(root.join("segments.jsonl"))?);
    let mut wav = BufWriter::new(
        OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(root.join("chapter.partial.wav"))?,
    );
    wav.write_all(&wav_header(0))?;
    let mut total = 0u64;
    let run = Instant::now();
    for (i, s) in plan.spec().segments.iter().enumerate() {
        let text = s.spoken.slice(&plan.spec().spoken)?;
        let start = Instant::now();
        let mut first = None;
        let mut count = 0;
        let request = Request {
            epoch: 1,
            plan: plan.id(),
            segment: i as u32,
            text,
            instruction: "A calm, clear English narrator.",
            seed: 42,
            max_frames: 1_440_000,
        };
        let result =
            provider.generate_streamed(request, &mut cache, &Cancellation::default(), |c| {
                assert_eq!(c.first_frame, count);
                count += c.pcm.len() as u64 / 2;
                first.get_or_insert(start.elapsed());
                Ok(())
            });
        let key = match result {
            Ok(k) => k,
            Err(e) => {
                writeln!(
                    rows,
                    "{}",
                    json!({"segment":i,"status":"failed","error":e.to_string(),"source":s.source})
                )?;
                rows.flush()?;
                return Err(e.into());
            }
        };
        let wall = start.elapsed().as_secs_f64();
        let audio = cache.get(key)?;
        let frames = audio.manifest().frames;
        if first.is_some() && count != frames {
            return Err("stream/cache frame mismatch".into());
        }
        let mut sum = 0f64;
        let mut peak = 0i32;
        let mut clipped = 0;
        let mut onset = None;
        let mut last = None;
        for (j, b) in audio.pcm().chunks_exact(2).enumerate() {
            let x = i16::from_le_bytes([b[0], b[1]]) as i32;
            peak = peak.max(x.abs());
            sum += (x as f64).powi(2);
            if x.abs() >= 32767 {
                clipped += 1;
            }
            if x.abs() > 104 {
                onset.get_or_insert(j);
                last = Some(j);
            }
        }
        wav.write_all(audio.pcm())?;
        let row = json!({"segment":i,"source":s.source,"key":key,"audio_hash":audio.manifest().audio_hash,"frames":frames,"chapter_first_frame":total,"wall_seconds":wall,"first_pcm_seconds":first.map(|d|d.as_secs_f64()),"cache_hit":first.is_none(),"rtf":wall/(frames as f64/24000.),"peak":peak,"rms":(sum/frames as f64).sqrt(),"clipped_samples":clipped,"onset_frame_threshold_50db":onset,"last_active_frame_threshold_50db":last,"status":"normal_eos"});
        writeln!(rows, "{row}")?;
        rows.flush()?;
        total += frames;
        println!(
            "segment={}/{} audio_seconds={:.2} total_audio_seconds={:.2} rtf={:.3}",
            i + 1,
            plan.spec().segments.len(),
            frames as f64 / 24000.,
            total as f64 / 24000.,
            wall / (frames as f64 / 24000.)
        );
    }
    provider.stop()?;
    wav.flush()?;
    wav.seek(SeekFrom::Start(0))?;
    wav.write_all(&wav_header(total))?;
    wav.flush()?;
    wav.get_ref().sync_all()?;
    drop(wav);
    fs::rename(root.join("chapter.partial.wav"), root.join("chapter.wav"))?;
    fs::write(
        root.join("completed.json"),
        serde_json::to_vec_pretty(
            &json!({"status":"engineering_render_complete_not_quality_pass","segments":plan.spec().segments.len(),"frames":total,"audio_seconds":total as f64/24000.,"wall_seconds":run.elapsed().as_secs_f64(),"source_coverage":"exact_contiguous_chapter","transcript_accuracy":"unmeasured","human_listening":"pending","voice_consistency":"pending","playback_underruns":"not_a_live_playback_test"}),
        )?,
    )?;
    println!("complete audio_seconds={:.2}", total as f64 / 24000.);
    Ok(())
}
