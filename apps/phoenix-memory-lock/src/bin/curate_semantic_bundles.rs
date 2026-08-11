use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[path = "semantic_curator/types.rs"]
mod types;

use anyhow::{bail, Context, Result};
use hashbrown::HashSet;
use keyring::Entry;
use reqwest::{
    header::{HeaderMap, HeaderValue, CONTENT_TYPE},
    Client,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio::time::Instant;
use types::*;

const PACKET_CONTRACT: &str = "phoenix.qps.semantic-review-bundle-batch/v1";
const DECISIONS_CONTRACT: &str = "phoenix.qps.relevance-review-decisions/v1";
const CACHE_CONTRACT: &str = "phoenix.qps.google-ai-semantic-review-cache/v3";
const RECEIPT_CONTRACT: &str = "phoenix.qps.google-ai-semantic-curation/v3";
const PROMPT_VERSION: &str = "phoenix.qps.semantic-adjudication-bidirectional-graded/v8";
const LEGACY_PAIRWISE_PROMPT_VERSION: &str =
    "phoenix.qps.semantic-adjudication-bidirectional-graded/v7";
const PROVIDER: &str = "google-ai-studio";
const SERVICE: &str = "Phoenix QPS V3 Semantic Curator";
const GOOGLE_AI_USER: &str = "google-ai-studio-api-key";
const GOOGLE_AI_KEY_ENV: &str = "PHOENIX_GOOGLE_AI_STUDIO_API_KEY";
const GOOGLE_API_ROOT: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const ALLOWED_MODEL: &str = "gemini-3.5-flash-lite";
const MAX_OUTPUT_TOKENS: u32 = 8_192;
const AUTHORIZATION: &str = "User explicitly authorized continued grouped semantic curation through the QPS V3 promotion corpus and canonical Phase 6-8 chain.";
const TECHNICAL_REASONS: [&str; 11] = [
    "partial_match_saturation",
    "scattered_terms",
    "phrase_order_failure",
    "identifier_collision",
    "fuzzy_collision",
    "weak_field_evidence",
    "common_term_dominance",
    "length_prior_failure",
    "wrong_concept_proximity",
    "document_conversation_confusion",
    "long_query_failure",
];

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("phoenix-v3-semantic-curator: {error:#}");
        std::process::exit(2);
    }
}

async fn run() -> Result<()> {
    let args = Args::parse(env::args().skip(1))?;
    let packet_path = args.required_path("--packet")?;
    let existing_dir = args.required_path("--existing-decisions-dir")?;
    let pairwise_cache_root = args.optional_path("--pairwise-cache-root");
    let output_root = args.required_path("--output-root")?;
    let decisions_output = args.required_path("--decisions-output")?;
    let model = args.required("--model")?.to_owned();
    let concurrency = args.usize_or("--concurrency", 4)?;
    let requests_per_minute = args.usize_or("--requests-per-minute", 14)?;
    let bundles_per_request = args.usize_or("--bundles-per-request", 4)?;
    let bundle_offset = args.usize_or("--bundle-offset", 0)?;
    let dataset_filter = args.optional_string("--dataset");
    let max_bundles = args.optional_usize("--max-bundles")?;
    let confidence_floor = args.f64_or("--confidence-floor", 0.70)?;
    let pre_cut_size = args.usize_or("--pre-cut-size", 900)?;
    let pre_cut_count = args.usize_or("--pre-cut-count", 4)?;
    validate_config(
        concurrency,
        requests_per_minute,
        bundles_per_request,
        confidence_floor,
        pre_cut_size,
        pre_cut_count,
    )?;

    let packet_bytes = fs::read(&packet_path)
        .with_context(|| format!("read packet at {}", packet_path.display()))?;
    let packet: BundleBatch =
        serde_json::from_slice(&packet_bytes).context("parse semantic bundle packet")?;
    if packet.contract != PACKET_CONTRACT || packet.schema_version != 1 {
        bail!("unsupported semantic bundle packet");
    }

    let existing = load_existing_decisions(&existing_dir)?;
    let mut remaining = Vec::with_capacity(packet.bundles.len());
    for mut bundle in packet.bundles {
        if dataset_filter
            .as_deref()
            .is_some_and(|dataset| bundle.dataset != dataset)
        {
            continue;
        }
        bundle
            .challengers
            .retain(|challenger| !existing.contains(&challenger.judgment_identity));
        if !bundle.challengers.is_empty() {
            remaining.push(bundle);
        }
    }
    let before_semantic_dedup = remaining.len();
    let mut seen_evidence_sets = HashSet::with_capacity(remaining.len());
    remaining.retain(|bundle| seen_evidence_sets.insert(bundle_evidence_identity(bundle)));
    let semantic_duplicate_bundles_skipped = before_semantic_dedup - remaining.len();
    let mut seen_evidence_cores = HashSet::with_capacity(remaining.len());
    let mut admitted_bundle_identities = HashSet::with_capacity(remaining.len());
    for bundle in &remaining {
        if seen_evidence_cores.insert(bundle_evidence_core_identity(bundle)) {
            admitted_bundle_identities.insert(bundle.bundle_identity.clone());
        }
    }
    let duplicate_evidence_bundles_rejected = remaining.len() - admitted_bundle_identities.len();
    let candidate_bundles_available = remaining.len();
    if bundle_offset >= remaining.len() {
        bail!(
            "bundle offset {bundle_offset} exceeds {} available bundles",
            remaining.len()
        );
    }
    remaining.drain(..bundle_offset);
    if let Some(limit) = max_bundles {
        remaining.truncate(limit);
    }
    if remaining.is_empty() {
        bail!("no unreviewed bundle challengers remain");
    }

    fs::create_dir_all(&output_root)
        .with_context(|| format!("create output root at {}", output_root.display()))?;
    let response_dir = output_root.join("responses");
    fs::create_dir_all(&response_dir)
        .with_context(|| format!("create response cache at {}", response_dir.display()))?;

    validate_model(&model)?;
    let api_key = load_google_ai_key()?;
    let client = google_client(&api_key).context("build Google AI Studio client")?;
    let rate_gate = Arc::new(RateGate::new(requests_per_minute));

    let work = remaining
        .chunks(bundles_per_request)
        .enumerate()
        .map(|(index, bundles)| RequestWork {
            index,
            bundles: bundles.to_vec(),
        })
        .collect::<Vec<_>>();
    let request_count = work.len();
    let mut next = 0usize;
    let mut complete = 0usize;
    let mut reviews = Vec::with_capacity(request_count);
    let mut failures = Vec::new();
    let mut tasks = JoinSet::new();

    while next < work.len() && tasks.len() < concurrency {
        spawn_request(
            &mut tasks,
            client.clone(),
            model.clone(),
            response_dir.clone(),
            pairwise_cache_root.clone(),
            rate_gate.clone(),
            work[next].clone(),
        );
        next += 1;
    }
    while let Some(joined) = tasks.join_next().await {
        match joined.context("join semantic review request")? {
            Ok(review) => reviews.push(review),
            Err(error) => failures.push(format!("{error:#}")),
        }
        complete += 1;
        println!("semantic_requests_complete={complete}/{request_count}");
        if next < work.len() {
            spawn_request(
                &mut tasks,
                client.clone(),
                model.clone(),
                response_dir.clone(),
                pairwise_cache_root.clone(),
                rate_gate.clone(),
                work[next].clone(),
            );
            next += 1;
        }
    }
    if !failures.is_empty() {
        let failure_path = output_root.join("failures.json");
        write_create_only_json(
            &failure_path,
            &json!({
                "contract": "phoenix.qps.google-ai-semantic-curation-failures/v1",
                "model": model,
                "failures": failures,
            }),
        )?;
        bail!(
            "{} semantic requests failed; rerun after inspecting {}",
            failures.len(),
            failure_path.display()
        );
    }

    reviews.sort_by_key(|review| review.request_index);
    let model_call_count = reviews
        .iter()
        .map(|review| review.model_call_count)
        .sum::<usize>();
    let pairwise_cache_reused_requests = reviews
        .iter()
        .filter(|review| review.pairwise_prompt_version == LEGACY_PAIRWISE_PROMPT_VERSION)
        .count();
    let mut decisions = Vec::new();
    let mut abstained = 0usize;
    let mut low_confidence = 0usize;
    let mut disagreements = 0usize;
    let mut grade_rejected = 0usize;
    let mut semantic_rejected = 0usize;
    let mut positive_evidence_rejected = 0usize;
    let mut duplicate_evidence_pair_rejected = 0usize;
    let mut positive = 0usize;
    let mut negative = 0usize;
    for cached in &reviews {
        for bundle_review in &cached.forward_review.bundles {
            let bundle = cached
                .bundles
                .get(bundle_review.bundle_slot)
                .context("validated bundle slot disappeared")?;
            if !admitted_bundle_identities.contains(&bundle.bundle_identity) {
                duplicate_evidence_pair_rejected += bundle.challengers.len();
                continue;
            }
            let positive_evidence = cached
                .positive_evidence_review
                .bundles
                .iter()
                .find(|review| review.bundle_slot == bundle_review.bundle_slot)
                .context("positive-evidence review bundle disappeared")?;
            if !positive_evidence
                .admits(confidence_floor, !bundle.reference_answer.trim().is_empty())
            {
                positive_evidence_rejected += bundle.challengers.len();
                continue;
            }
            let reverse_bundle = cached
                .reverse_review
                .bundles
                .iter()
                .find(|review| review.bundle_slot == bundle_review.bundle_slot)
                .context("reverse review bundle disappeared")?;
            for adjudication in &bundle_review.decisions {
                let challenger = bundle
                    .challengers
                    .get(adjudication.challenger_slot)
                    .context("validated challenger slot disappeared")?;
                let reverse = reverse_bundle
                    .decisions
                    .iter()
                    .find(|review| review.challenger_slot == adjudication.challenger_slot)
                    .context("reverse challenger adjudication disappeared")?;
                let forward_verdict = adjudication.verdict.as_pair(false);
                let reverse_verdict = reverse.verdict.as_pair(true);
                match (forward_verdict, reverse_verdict) {
                    (None, _) | (_, None) => abstained += 1,
                    (Some(forward), Some(reverse_verdict)) if forward != reverse_verdict => {
                        disagreements += 1;
                    }
                    (Some(verdict), Some(_)) => {
                        let confidence = adjudication.confidence.min(reverse.confidence);
                        if confidence < confidence_floor {
                            low_confidence += 1;
                            continue;
                        }
                        if !grades_support_verdict(adjudication, reverse, verdict) {
                            grade_rejected += 1;
                            continue;
                        }
                        if !semantic_guards_allow(bundle, adjudication, reverse, verdict) {
                            semantic_rejected += 1;
                            continue;
                        }
                        if verdict == PairVerdict::PositivePreferred {
                            positive += 1;
                        } else {
                            negative += 1;
                        }
                        validate_reason(&challenger.suggested_reason)?;
                        decisions.push(Decision {
                            judgment_identity: challenger.judgment_identity.clone(),
                            verdict: verdict.as_contract().to_owned(),
                            reason: challenger.suggested_reason.clone(),
                            source: "curated_regression_case".to_owned(),
                            confidence,
                        });
                    }
                }
            }
        }
    }
    let expected_pairs = remaining
        .iter()
        .map(|bundle| bundle.challengers.len())
        .sum::<usize>();
    if decisions.len()
        + abstained
        + low_confidence
        + disagreements
        + grade_rejected
        + semantic_rejected
        + positive_evidence_rejected
        + duplicate_evidence_pair_rejected
        != expected_pairs
    {
        bail!("semantic accounting mismatch");
    }

    let reviewed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock precedes Unix epoch")?
        .as_secs();
    let reviewer = reviewer_identity(&model);
    let cut_paths = write_decision_cuts(
        &decisions_output,
        &reviewer,
        reviewed_at,
        &decisions,
        pre_cut_size,
        pre_cut_count,
    )?;
    let decision_cuts = cut_paths
        .iter()
        .map(|path| file_identity(Path::new(path)))
        .collect::<Result<Vec<_>>>()?;
    let producer_binary = file_identity(&env::current_exe().context("resolve curator binary")?)?;
    let receipt = CurationReceipt {
        contract: RECEIPT_CONTRACT,
        schema_version: 2,
        provider: PROVIDER,
        prompt_version: PROMPT_VERSION,
        producer_binary,
        packet_path: packet_path.display().to_string(),
        packet_sha256: hex_sha256(&packet_bytes),
        model,
        reviewer_identity: reviewer,
        authorization_context: AUTHORIZATION,
        existing_decisions_skipped: existing.len(),
        semantic_duplicate_bundles_skipped,
        duplicate_evidence_bundles_rejected,
        candidate_bundles_available,
        bundle_offset,
        dataset_filter,
        bundles_reviewed: remaining.len(),
        pairs_reviewed: expected_pairs,
        positive_preferred: positive,
        negative_preferred: negative,
        abstained,
        below_confidence_floor: low_confidence,
        bidirectional_disagreements: disagreements,
        grade_guard_rejections: grade_rejected,
        semantic_guard_rejections: semantic_rejected,
        positive_evidence_rejections: positive_evidence_rejected,
        duplicate_evidence_pair_rejections: duplicate_evidence_pair_rejected,
        decisive_decisions: decisions.len(),
        confidence_floor,
        requests_per_minute,
        pairwise_cache_root: pairwise_cache_root.map(|path| path.display().to_string()),
        pairwise_cache_reused_requests,
        request_count,
        model_call_count,
        decision_cuts,
        forbidden_inputs_exposed: false,
    };
    let receipt_path = output_root.join("curation-receipt.json");
    write_create_only_json(&receipt_path, &receipt)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}

fn spawn_request(
    tasks: &mut JoinSet<Result<CachedReview>>,
    client: Client,
    model: String,
    response_dir: PathBuf,
    pairwise_cache_root: Option<PathBuf>,
    rate_gate: Arc<RateGate>,
    work: RequestWork,
) {
    tasks.spawn(async move {
        review_request(
            &client,
            &model,
            &response_dir,
            pairwise_cache_root.as_deref(),
            &rate_gate,
            work,
        )
        .await
    });
}

async fn review_request(
    client: &Client,
    model: &str,
    response_dir: &Path,
    pairwise_cache_root: Option<&Path>,
    rate_gate: &RateGate,
    work: RequestWork,
) -> Result<CachedReview> {
    let cache_path = response_dir.join(format!(
        "{}.json",
        request_identity(PROMPT_VERSION, model, &work)
    ));
    if cache_path.exists() {
        let cached: CachedReview = serde_json::from_slice(&fs::read(&cache_path)?)?;
        validate_cached(&cached, model, &work)?;
        return Ok(cached);
    }

    let positive_evidence_review =
        review_positive_evidence_once(client, model, &work.bundles, rate_gate).await?;
    let legacy_pairwise = pairwise_cache_root
        .map(|root| load_legacy_pairwise_cache(root, model, &work))
        .transpose()?;
    let (pairwise_prompt_version, model_call_count, forward_review, reverse_review) =
        if let Some((forward, reverse)) = legacy_pairwise {
            (
                LEGACY_PAIRWISE_PROMPT_VERSION.to_owned(),
                1,
                forward,
                reverse,
            )
        } else {
            (
                PROMPT_VERSION.to_owned(),
                3,
                review_once(client, model, &work.bundles, rate_gate, false, 0).await?,
                review_once(client, model, &work.bundles, rate_gate, true, 1).await?,
            )
        };
    let cached = CachedReview {
        contract: CACHE_CONTRACT.to_owned(),
        prompt_version: PROMPT_VERSION.to_owned(),
        model: model.to_owned(),
        pairwise_prompt_version,
        model_call_count,
        request_index: work.index,
        bundles: work.bundles,
        positive_evidence_review,
        forward_review,
        reverse_review,
    };
    write_create_only_json(&cache_path, &cached)?;
    Ok(cached)
}

async fn review_positive_evidence_once(
    client: &Client,
    model: &str,
    bundles: &[QueryBundle],
    rate_gate: &RateGate,
) -> Result<PositiveEvidenceReview> {
    let prompt = build_positive_evidence_prompt(bundles)?;
    let schema = positive_evidence_schema();
    let mut last_error = None;
    for attempt in 0..4u32 {
        println!(
            "semantic_model_call model={model} direction=positive-evidence attempt={}/4 bundles={}",
            attempt + 1,
            bundles.len()
        );
        rate_gate.acquire().await;
        let request =
            google_request_with_system(&prompt, &schema, 2, positive_evidence_system_prompt());
        let endpoint = format!("{GOOGLE_API_ROOT}/{model}:generateContent");
        let response = tokio::time::timeout(
            Duration::from_secs(90),
            client.post(endpoint).json(&request).send(),
        )
        .await;
        let parsed = match response {
            Ok(Ok(response)) => parse_google_response::<PositiveEvidenceReview>(response).await,
            Ok(Err(error)) => {
                Err(anyhow::anyhow!(error).context("Google AI positive-evidence review request"))
            }
            Err(_) => Err(anyhow::anyhow!(
                "Google AI positive-evidence review timed out after 90 seconds"
            )),
        };
        let rate_limited = parsed
            .as_ref()
            .err()
            .is_some_and(|error| format!("{error:#}").contains("HTTP 429"));
        match parsed.and_then(|review| {
            validate_positive_evidence_review(&review, bundles)?;
            Ok(review)
        }) {
            Ok(review) => return Ok(review),
            Err(error) => {
                eprintln!(
                    "semantic_model_call_failed model={model} direction=positive-evidence attempt={}: {error:#}",
                    attempt + 1
                );
                last_error = Some(error);
            }
        }
        let delay = if rate_limited {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(1u64 << attempt)
        };
        tokio::time::sleep(delay).await;
    }
    Err(last_error.context("positive-evidence review failed without an error")?)
}

async fn review_once(
    client: &Client,
    model: &str,
    bundles: &[QueryBundle],
    rate_gate: &RateGate,
    reversed: bool,
    seed: u32,
) -> Result<ModelReview> {
    let prompt = build_prompt(bundles, reversed)?;
    let schema = response_schema();
    let mut last_error = None;
    for attempt in 0..4u32 {
        println!(
            "semantic_model_call model={model} direction={} attempt={}/4 bundles={}",
            if reversed { "reverse" } else { "forward" },
            attempt + 1,
            bundles.len()
        );
        rate_gate.acquire().await;
        let request = google_request(&prompt, &schema, seed);
        let endpoint = format!("{GOOGLE_API_ROOT}/{model}:generateContent");
        let response = tokio::time::timeout(
            Duration::from_secs(90),
            client.post(endpoint).json(&request).send(),
        )
        .await;
        let parsed = match response {
            Ok(Ok(response)) => parse_google_response(response).await,
            Ok(Err(error)) => {
                Err(anyhow::anyhow!(error).context("Google AI semantic review request"))
            }
            Err(_) => Err(anyhow::anyhow!(
                "Google AI semantic review timed out after 90 seconds"
            )),
        };
        let rate_limited = parsed
            .as_ref()
            .err()
            .is_some_and(|error| format!("{error:#}").contains("HTTP 429"));
        match parsed.and_then(|review| {
            validate_review(&review, bundles)?;
            Ok(review)
        }) {
            Ok(review) => return Ok(review),
            Err(error) => {
                eprintln!(
                    "semantic_model_call_failed model={model} direction={} attempt={}: {error:#}",
                    if reversed { "reverse" } else { "forward" },
                    attempt + 1
                );
                last_error = Some(error);
            }
        }
        let delay = if rate_limited {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(1u64 << attempt)
        };
        tokio::time::sleep(delay).await;
    }
    Err(last_error.context("semantic review failed without an error")?)
}

struct RateGate {
    next_request: Mutex<Instant>,
    interval: Duration,
}

impl RateGate {
    fn new(requests_per_minute: usize) -> Self {
        Self {
            next_request: Mutex::new(Instant::now()),
            interval: Duration::from_secs_f64(60.0 / requests_per_minute as f64),
        }
    }

    async fn acquire(&self) {
        let mut next = self.next_request.lock().await;
        let now = Instant::now();
        if *next > now {
            tokio::time::sleep_until(*next).await;
        }
        *next = Instant::now() + self.interval;
    }
}

fn google_request(prompt: &str, schema: &Value, seed: u32) -> Value {
    google_request_with_system(prompt, schema, seed, system_prompt())
}

fn google_request_with_system(prompt: &str, schema: &Value, seed: u32, system: &str) -> Value {
    json!({
        "systemInstruction": {
            "parts": [{"text": system}]
        },
        "contents": [{
            "role": "user",
            "parts": [{"text": prompt}]
        }],
        "generationConfig": {
            "temperature": 0.0,
            "candidateCount": 1,
            "maxOutputTokens": MAX_OUTPUT_TOKENS,
            "seed": seed,
            "responseMimeType": "application/json",
            "responseJsonSchema": schema
        }
    })
}

async fn parse_google_response<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    let body = response
        .bytes()
        .await
        .context("read Google AI semantic review response")?;
    if !status.is_success() {
        let message = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "Google AI request failed without a message".to_owned());
        bail!("Google AI returned HTTP {status}: {message}");
    }
    let value: Value =
        serde_json::from_slice(&body).context("parse Google AI response envelope")?;
    let content = value
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(Value::as_str)
        .context("Google AI returned no semantic review content")?;
    parse_review(content)
}

fn grades_support_verdict(
    forward: &Adjudication,
    reverse: &Adjudication,
    verdict: PairVerdict,
) -> bool {
    let (forward_positive, forward_negative) = forward.pair_relevance(false);
    let (reverse_positive, reverse_negative) = reverse.pair_relevance(true);
    if forward_positive.abs_diff(reverse_positive) > 1
        || forward_negative.abs_diff(reverse_negative) > 1
    {
        return false;
    }
    match verdict {
        PairVerdict::PositivePreferred => {
            forward_positive >= 3
                && reverse_positive >= 3
                && forward_positive > forward_negative
                && reverse_positive > reverse_negative
        }
        PairVerdict::NegativePreferred => {
            forward_negative >= 3
                && reverse_negative >= 3
                && forward_negative > forward_positive
                && reverse_negative > reverse_positive
        }
    }
}

fn semantic_guards_allow(
    bundle: &QueryBundle,
    forward: &Adjudication,
    reverse: &Adjudication,
    verdict: PairVerdict,
) -> bool {
    if !forward.input_consistent_and_answerable
        || !reverse.input_consistent_and_answerable
        || !forward.query_discriminates_candidates
        || !reverse.query_discriminates_candidates
        || forward.candidates_cover_disjoint_valid_facets
        || reverse.candidates_cover_disjoint_valid_facets
        || forward.same_or_versioned_evidence
        || reverse.same_or_versioned_evidence
    {
        return false;
    }
    let (forward_positive_support, forward_negative_support) = forward.pair_support(false);
    let (reverse_positive_support, reverse_negative_support) = reverse.pair_support(true);
    let (forward_positive_inference, forward_negative_inference) =
        forward.pair_unsupported_inference(false);
    let (reverse_positive_inference, reverse_negative_inference) =
        reverse.pair_unsupported_inference(true);
    let winner_is_fully_supported = match verdict {
        PairVerdict::PositivePreferred => {
            forward_positive_support
                && reverse_positive_support
                && !forward_positive_inference
                && !reverse_positive_inference
        }
        PairVerdict::NegativePreferred => {
            forward_negative_support
                && reverse_negative_support
                && !forward_negative_inference
                && !reverse_negative_inference
        }
    };
    if !winner_is_fully_supported {
        return false;
    }
    let query_terms = bundle
        .query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .count();
    if !bundle.reference_answer.trim().is_empty() || query_terms > 3 {
        return true;
    }
    let (forward_positive, forward_negative) = forward.pair_relevance(false);
    let (reverse_positive, reverse_negative) = reverse.pair_relevance(true);
    match verdict {
        PairVerdict::PositivePreferred => {
            forward_positive >= 3
                && reverse_positive >= 3
                && forward_negative == 0
                && reverse_negative == 0
        }
        PairVerdict::NegativePreferred => {
            forward_negative >= 3
                && reverse_negative >= 3
                && forward_positive == 0
                && reverse_positive == 0
        }
    }
}

fn positive_evidence_system_prompt() -> &'static str {
    "You are a strict retrieval gold-evidence auditor. Judge the supplied positive candidate by itself, never relative to another document. Atomize every optional reference answer into independently required facts and count them. Mark all_requested_parts_supported true only when every requested or reference component is explicitly supported. Verify that every named query entity matches the candidate speaker, title, or text; near-spellings such as Andrew versus Audrey are different people unless the evidence explicitly equates them. Mark input_consistent_and_answerable false when the query and reference conflict, are malformed, or do not define a coherent information need. For a question, the candidate must explicitly answer it. For a declarative scientific claim, directly confirming or refuting evidence can satisfy the information need; it need not affirm the claim. Mark positive_requires_unsupported_inference true when support depends on an unstated relation, date, chronology, identity, location, purchase, genre, cause, or guessed fact. Topic overlap and partial coverage are insufficient. Return every bundle slot exactly once and keep rationales under 24 words."
}

fn build_positive_evidence_prompt(bundles: &[QueryBundle]) -> Result<String> {
    let public = bundles
        .iter()
        .enumerate()
        .map(|(bundle_slot, bundle)| {
            json!({
                "bundle_slot": bundle_slot,
                "task_context": retrieval_task_context(&bundle.dataset),
                "query": bundle.query,
                "reference_answer": bundle.reference_answer,
                "positive_candidate": public_document(&bundle.positive),
            })
        })
        .collect::<Vec<_>>();
    Ok(format!(
        "Audit whether each positive candidate independently establishes the complete information need. Count all atomic reference components, verify every named entity, and do not compare with any unseen alternative. Inputs:\n{}",
        serde_json::to_string(&public)?
    ))
}

fn positive_evidence_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["bundles"],
        "properties": {
            "bundles": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["bundle_slot", "input_consistent_and_answerable", "positive_fully_supports_information_need", "positive_requires_unsupported_inference", "query_entities_match_positive", "all_requested_parts_supported", "reference_component_count", "supported_reference_component_count", "confidence", "rationale"],
                    "properties": {
                        "bundle_slot": {"type": "integer", "minimum": 0},
                        "input_consistent_and_answerable": {"type": "boolean"},
                        "positive_fully_supports_information_need": {"type": "boolean"},
                        "positive_requires_unsupported_inference": {"type": "boolean"},
                        "query_entities_match_positive": {"type": "boolean"},
                        "all_requested_parts_supported": {"type": "boolean"},
                        "reference_component_count": {"type": "integer", "minimum": 0, "maximum": 16},
                        "supported_reference_component_count": {"type": "integer", "minimum": 0, "maximum": 16},
                        "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                        "rationale": {"type": "string", "maxLength": 240}
                    }
                }
            }
        }
    })
}

fn validate_positive_evidence_review(
    review: &PositiveEvidenceReview,
    bundles: &[QueryBundle],
) -> Result<()> {
    if review.bundles.len() != bundles.len() {
        bail!("model returned the wrong positive-evidence bundle count");
    }
    let mut slots = HashSet::with_capacity(review.bundles.len());
    for decision in &review.bundles {
        if bundles.get(decision.bundle_slot).is_none()
            || !slots.insert(decision.bundle_slot)
            || !decision.confidence.is_finite()
            || !(0.0..=1.0).contains(&decision.confidence)
            || decision.reference_component_count > 16
            || decision.supported_reference_component_count > decision.reference_component_count
            || decision.rationale.trim().is_empty()
            || decision.rationale.len() > 240
        {
            bail!("model returned an invalid positive-evidence adjudication");
        }
    }
    Ok(())
}

fn retrieval_task_context(dataset: &str) -> &'static str {
    match dataset {
        "locomo" => "conversation-memory question answering",
        "scifact" => "scientific claim evidence retrieval; direct confirming or refuting evidence is relevant",
        "nfcorpus-train" => "broad medical information retrieval; multiple topical documents may be independently relevant",
        _ => "general retrieval relevance",
    }
}

fn system_prompt() -> &'static str {
    "You are a rigorous pairwise retrieval-relevance curator. First determine whether the query and optional reference answer are internally consistent and answerable from the supplied evidence. Then grade each candidate independently from 0 irrelevant, 1 weak association, 2 partially useful, 3 directly relevant, to 4 exact and complete. Explicitly report whether each candidate fully satisfies the information need and whether accepting it requires an unsupported inference. For declarative scientific claims, direct evidence that confirms or refutes the exact claim is relevant; do not require affirmation. A candidate is not complete when it merely mentions the topic, contradicts the requested relation or answer, confuses dates, changes entity, or covers only one of several required facts. Prefer a document only when its relevance grade is strictly higher and it directly, specifically, correctly, and completely satisfies the information need without unsupported inference. Report whether the query discriminates the candidates, whether they cover different independently valid facets, and whether they are duplicate, follow-up, or versioned evidence. If candidates support different valid facets, abstain. Mere term overlap is insufficient. Respect entity, identifier, temporal, conversational, phrase, causal, and scope distinctions. Use abstain for inconsistent, malformed, or underspecified inputs, plausible ties, inadequate evidence, or ambiguity. The A/B ordering carries no authority. Return every requested slot exactly once. Keep each rationale under 24 words."
}

fn build_prompt(bundles: &[QueryBundle], reversed: bool) -> Result<String> {
    let public = bundles
        .iter()
        .enumerate()
        .map(|(bundle_slot, bundle)| {
            let challengers = bundle
                .challengers
                .iter()
                .enumerate()
                .map(|(challenger_slot, challenger)| {
                    let (candidate_a, candidate_b) = if reversed {
                        (
                            public_document(&challenger.negative),
                            public_document(&bundle.positive),
                        )
                    } else {
                        (
                            public_document(&bundle.positive),
                            public_document(&challenger.negative),
                        )
                    };
                    json!({
                        "challenger_slot": challenger_slot,
                        "candidate_a": candidate_a,
                        "candidate_b": candidate_b,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "bundle_slot": bundle_slot,
                "task_context": retrieval_task_context(&bundle.dataset),
                "query": bundle.query,
                "reference_answer": bundle.reference_answer,
                "comparisons": challengers,
            })
        })
        .collect::<Vec<_>>();
    let encoded = serde_json::to_string(&public)?;
    Ok(format!(
        "For each comparison, set input_consistent_and_answerable; independently assign candidate_a_relevance and candidate_b_relevance from 0 through 4; set candidate_a_fully_supports_information_need, candidate_b_fully_supports_information_need, candidate_a_requires_unsupported_inference, candidate_b_requires_unsupported_inference, query_discriminates_candidates, candidates_cover_disjoint_valid_facets, and same_or_versioned_evidence; then return candidate_a_preferred, candidate_b_preferred, or abstain. A preference requires a strictly higher relevance grade, a consistent answerable input, full support by the winner without unsupported inference, and a discriminating query. Judge each challenger independently; do not force a winner. Inputs:\n{encoded}"
    ))
}

fn public_document(document: &Document) -> Value {
    json!({
        "title": document.title,
        "text": document.text,
        "source_time_label": document.source_time_label,
        "reviewer_context": document.reviewer_context,
    })
}

fn response_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["bundles"],
        "properties": {
            "bundles": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["bundle_slot", "decisions"],
                    "properties": {
                        "bundle_slot": {"type": "integer", "minimum": 0},
                        "decisions": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["challenger_slot", "verdict", "candidate_a_relevance", "candidate_b_relevance", "candidate_a_fully_supports_information_need", "candidate_b_fully_supports_information_need", "candidate_a_requires_unsupported_inference", "candidate_b_requires_unsupported_inference", "input_consistent_and_answerable", "query_discriminates_candidates", "candidates_cover_disjoint_valid_facets", "same_or_versioned_evidence", "confidence", "rationale"],
                                "properties": {
                                    "challenger_slot": {"type": "integer", "minimum": 0},
                                    "verdict": {"type": "string", "enum": ["candidate_a_preferred", "candidate_b_preferred", "abstain"]},
                                    "candidate_a_relevance": {"type": "integer", "minimum": 0, "maximum": 4},
                                    "candidate_b_relevance": {"type": "integer", "minimum": 0, "maximum": 4},
                                    "candidate_a_fully_supports_information_need": {"type": "boolean"},
                                    "candidate_b_fully_supports_information_need": {"type": "boolean"},
                                    "candidate_a_requires_unsupported_inference": {"type": "boolean"},
                                    "candidate_b_requires_unsupported_inference": {"type": "boolean"},
                                    "input_consistent_and_answerable": {"type": "boolean"},
                                    "query_discriminates_candidates": {"type": "boolean"},
                                    "candidates_cover_disjoint_valid_facets": {"type": "boolean"},
                                    "same_or_versioned_evidence": {"type": "boolean"},
                                    "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                                    "rationale": {"type": "string", "maxLength": 240}
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

fn parse_review<T: DeserializeOwned>(content: &str) -> Result<T> {
    let trimmed = content.trim();
    let json = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .strip_suffix("```")
        .unwrap_or(trimmed)
        .trim();
    serde_json::from_str(json).context("parse structured semantic review")
}

fn validate_review(review: &ModelReview, bundles: &[QueryBundle]) -> Result<()> {
    if review.bundles.len() != bundles.len() {
        bail!("model returned the wrong bundle count");
    }
    let mut bundle_slots = HashSet::with_capacity(review.bundles.len());
    for bundle_review in &review.bundles {
        let bundle = bundles
            .get(bundle_review.bundle_slot)
            .context("model returned an invalid bundle slot")?;
        if !bundle_slots.insert(bundle_review.bundle_slot)
            || bundle_review.decisions.len() != bundle.challengers.len()
        {
            bail!("model returned duplicate or incomplete bundle decisions");
        }
        let mut challenger_slots = HashSet::with_capacity(bundle_review.decisions.len());
        for decision in &bundle_review.decisions {
            if bundle.challengers.get(decision.challenger_slot).is_none()
                || !challenger_slots.insert(decision.challenger_slot)
                || decision.candidate_a_relevance > 4
                || decision.candidate_b_relevance > 4
                || !decision.confidence.is_finite()
                || !(0.0..=1.0).contains(&decision.confidence)
                || decision.rationale.trim().is_empty()
                || decision.rationale.len() > 240
            {
                bail!("model returned an invalid challenger adjudication");
            }
        }
    }
    Ok(())
}

fn validate_cached(cached: &CachedReview, model: &str, work: &RequestWork) -> Result<()> {
    if cached.contract != CACHE_CONTRACT
        || cached.prompt_version != PROMPT_VERSION
        || cached.model != model
        || cached.request_index != work.index
        || cached.bundles != work.bundles
        || cached.model_call_count == 0
        || !matches!(
            cached.pairwise_prompt_version.as_str(),
            PROMPT_VERSION | LEGACY_PAIRWISE_PROMPT_VERSION
        )
    {
        bail!("cached semantic review binding mismatch");
    }
    validate_positive_evidence_review(&cached.positive_evidence_review, &cached.bundles)?;
    validate_review(&cached.forward_review, &cached.bundles)?;
    validate_review(&cached.reverse_review, &cached.bundles)
}

fn load_legacy_pairwise_cache(
    root: &Path,
    model: &str,
    work: &RequestWork,
) -> Result<(ModelReview, ModelReview)> {
    let path = root.join(format!(
        "{}.json",
        request_identity(LEGACY_PAIRWISE_PROMPT_VERSION, model, work)
    ));
    let cached: CachedReview = serde_json::from_slice(
        &fs::read(&path).with_context(|| format!("read pairwise cache at {}", path.display()))?,
    )
    .with_context(|| format!("parse pairwise cache at {}", path.display()))?;
    if cached.contract != "phoenix.qps.google-ai-semantic-review-cache/v2"
        || cached.prompt_version != LEGACY_PAIRWISE_PROMPT_VERSION
        || cached.model != model
        || cached.request_index != work.index
        || cached.bundles != work.bundles
    {
        bail!(
            "legacy pairwise cache binding mismatch at {}",
            path.display()
        );
    }
    validate_review(&cached.forward_review, &cached.bundles)?;
    validate_review(&cached.reverse_review, &cached.bundles)?;
    Ok((cached.forward_review, cached.reverse_review))
}

fn load_existing_decisions(dir: &Path) -> Result<HashSet<String>> {
    let mut identities = HashSet::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("qps-v3-semantic-review-decisions-") || !name.ends_with(".json") {
            continue;
        }
        let document: ExistingDecisionDocument = serde_json::from_slice(&fs::read(entry.path())?)
            .with_context(|| {
            format!("parse existing decisions at {}", entry.path().display())
        })?;
        if document.contract != DECISIONS_CONTRACT {
            bail!(
                "invalid existing decision contract at {}",
                entry.path().display()
            );
        }
        for decision in document.decisions {
            identities.insert(decision.judgment_identity);
        }
    }
    Ok(identities)
}

fn write_decision_cuts(
    base: &Path,
    reviewer: &str,
    reviewed_at: u64,
    decisions: &[Decision],
    pre_cut_size: usize,
    pre_cut_count: usize,
) -> Result<Vec<String>> {
    let required_pre = pre_cut_size
        .checked_mul(pre_cut_count)
        .context("pre-cut size overflow")?;
    if decisions.len() <= required_pre {
        bail!(
            "{} decisive decisions cannot fill {} pre-promotion cuts",
            decisions.len(),
            pre_cut_count
        );
    }
    let mut paths = Vec::with_capacity(pre_cut_count + 1);
    for cut in 0..pre_cut_count {
        let start = cut * pre_cut_size;
        let end = start + pre_cut_size;
        let path = numbered_path(base, cut + 1);
        write_decision_document(&path, reviewer, reviewed_at, &decisions[start..end])?;
        paths.push(path.display().to_string());
    }
    let final_path = numbered_path(base, pre_cut_count + 1);
    write_decision_document(
        &final_path,
        reviewer,
        reviewed_at,
        &decisions[required_pre..],
    )?;
    paths.push(final_path.display().to_string());
    Ok(paths)
}

fn write_decision_document(
    path: &Path,
    reviewer: &str,
    reviewed_at: u64,
    decisions: &[Decision],
) -> Result<()> {
    let document = DecisionDocument {
        contract: DECISIONS_CONTRACT,
        schema_version: 1,
        reviewer_identity: reviewer,
        reviewed_at_unix_seconds: reviewed_at,
        attestation: "agent_curated_with_user_authorization",
        authorization_context: AUTHORIZATION,
        decisions,
    };
    write_create_only_json(path, &document)
}

fn numbered_path(base: &Path, cut: usize) -> PathBuf {
    let parent = base.parent().unwrap_or_else(|| Path::new("."));
    let stem = base
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("decisions");
    parent.join(format!("{stem}-cut{cut:02}.json"))
}

fn write_create_only_json(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(value)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create output at {}", path.display()))?;
    output.write_all(&bytes)?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    Ok(())
}

fn google_client(api_key: &str) -> Result<Client> {
    let mut key = HeaderValue::from_str(api_key).context("encode Google AI credential")?;
    key.set_sensitive(true);
    let mut headers = HeaderMap::with_capacity(2);
    headers.insert("x-goog-api-key", key);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Client::builder()
        .default_headers(headers)
        .user_agent("Phoenix-QPS-V3-Semantic-Curator/1")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .context("construct reqwest client")
}

fn load_google_ai_key() -> Result<String> {
    let entry = Entry::new(SERVICE, GOOGLE_AI_USER).context("open Google AI credential")?;
    if let Ok(secret) = env::var(GOOGLE_AI_KEY_ENV) {
        validate_google_key(&secret)?;
        entry
            .set_password(&secret)
            .context("store Google AI credential")?;
        env::remove_var(GOOGLE_AI_KEY_ENV);
        return Ok(secret);
    }
    let secret = entry.get_password().context(
        "read Google AI credential; provision it once through PHOENIX_GOOGLE_AI_STUDIO_API_KEY",
    )?;
    validate_google_key(&secret)?;
    Ok(secret)
}

fn validate_google_key(secret: &str) -> Result<()> {
    if !secret.starts_with("AIza")
        || secret.len() < 32
        || !secret.bytes().all(|byte| byte.is_ascii_graphic())
    {
        bail!("Google AI credential has an invalid shape");
    }
    Ok(())
}

fn validate_model(model: &str) -> Result<()> {
    if model != ALLOWED_MODEL {
        bail!("this zero-cost curation cut only permits {ALLOWED_MODEL}");
    }
    Ok(())
}

fn validate_config(
    concurrency: usize,
    requests_per_minute: usize,
    bundles_per_request: usize,
    confidence_floor: f64,
    pre_cut_size: usize,
    pre_cut_count: usize,
) -> Result<()> {
    if !(1..=16).contains(&concurrency)
        || !(1..=14).contains(&requests_per_minute)
        || !(1..=8).contains(&bundles_per_request)
        || !confidence_floor.is_finite()
        || !(0.5..=1.0).contains(&confidence_floor)
        || pre_cut_size == 0
        || pre_cut_count == 0
    {
        bail!("invalid curator configuration");
    }
    Ok(())
}

fn validate_reason(reason: &str) -> Result<()> {
    if !TECHNICAL_REASONS.contains(&reason) {
        bail!("unsupported technical failure reason {reason}");
    }
    Ok(())
}

fn reviewer_identity(model: &str) -> String {
    format!(
        "google-ai-studio-{}-qps-v3-semantic-v8",
        model.replace(['/', ':'], "-")
    )
}

fn request_identity(prompt_version: &str, model: &str, work: &RequestWork) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt_version.as_bytes());
    hasher.update([0]);
    hasher.update(model.as_bytes());
    for bundle in &work.bundles {
        hasher.update([0]);
        hasher.update(bundle.bundle_identity.as_bytes());
        for challenger in &bundle.challengers {
            hasher.update([0]);
            hasher.update(challenger.judgment_identity.as_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
}

fn bundle_evidence_identity(bundle: &QueryBundle) -> [u8; 32] {
    let mut challenger_ids = bundle
        .challengers
        .iter()
        .map(|challenger| challenger.negative.id.as_str())
        .collect::<Vec<_>>();
    challenger_ids.sort_unstable();
    let mut hasher = Sha256::new();
    hasher.update(bundle.dataset.as_bytes());
    hasher.update([0]);
    hasher.update(bundle.reference_answer.trim().as_bytes());
    hasher.update([0]);
    hasher.update(bundle.positive.id.as_bytes());
    for challenger_id in challenger_ids {
        hasher.update([0]);
        hasher.update(challenger_id.as_bytes());
    }
    hasher.finalize().into()
}

fn bundle_evidence_core_identity(bundle: &QueryBundle) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bundle.dataset.as_bytes());
    hasher.update([0]);
    hasher.update(bundle.reference_answer.trim().as_bytes());
    hasher.update([0]);
    hasher.update(bundle.positive.id.as_bytes());
    hasher.finalize().into()
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn file_identity(path: &Path) -> Result<FileIdentity> {
    let bytes = fs::read(path).with_context(|| format!("read artifact at {}", path.display()))?;
    Ok(FileIdentity {
        path: path.display().to_string(),
        bytes: bytes.len() as u64,
        sha256: hex_sha256(&bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(text: &str) -> Document {
        Document {
            id: format!("id-{text}"),
            title: "title".to_owned(),
            text: text.to_owned(),
            source_time_label: String::new(),
            reviewer_context: String::new(),
        }
    }

    fn bundle() -> QueryBundle {
        QueryBundle {
            bundle_identity: "bundle-secret".to_owned(),
            dataset: "fixture".to_owned(),
            query_id: "query-secret".to_owned(),
            query: "which answer is right?".to_owned(),
            reference_answer: "the direct answer".to_owned(),
            positive: document("candidate a"),
            challengers: vec![Challenger {
                judgment_identity: "judgment-secret".to_owned(),
                negative: document("candidate b"),
                suggested_reason: "fuzzy_collision".to_owned(),
            }],
        }
    }

    #[test]
    fn prompt_excludes_identity_rank_feature_and_reason_labels() {
        let prompt = build_prompt(&[bundle()], false).unwrap();
        assert!(!prompt.contains("bundle-secret"));
        assert!(!prompt.contains("query-secret"));
        assert!(!prompt.contains("judgment-secret"));
        assert!(!prompt.contains("id-candidate"));
        assert!(!prompt.contains("fuzzy_collision"));
        assert!(!prompt.contains("v2_position"));
        assert!(prompt.contains("candidate a"));
        assert!(prompt.contains("candidate b"));
    }

    #[test]
    fn semantic_evidence_identity_ignores_duplicate_query_spelling() {
        let first = bundle();
        let mut typo = bundle();
        typo.bundle_identity = "other-bundle".to_owned();
        typo.query_id = "other-query".to_owned();
        typo.query = "which anser is right?".to_owned();
        assert_eq!(
            bundle_evidence_identity(&first),
            bundle_evidence_identity(&typo)
        );

        typo.challengers[0].negative.id = "different-negative".to_owned();
        assert_ne!(
            bundle_evidence_identity(&first),
            bundle_evidence_identity(&typo)
        );
        assert_eq!(
            bundle_evidence_core_identity(&first),
            bundle_evidence_core_identity(&typo)
        );
    }

    #[test]
    fn validation_requires_every_slot_once() {
        let review = ModelReview {
            bundles: vec![BundleReview {
                bundle_slot: 0,
                decisions: vec![Adjudication {
                    challenger_slot: 0,
                    verdict: CandidateVerdict::CandidateAPreferred,
                    candidate_a_relevance: 4,
                    candidate_b_relevance: 1,
                    candidate_a_fully_supports_information_need: true,
                    candidate_b_fully_supports_information_need: false,
                    candidate_a_requires_unsupported_inference: false,
                    candidate_b_requires_unsupported_inference: false,
                    input_consistent_and_answerable: true,
                    query_discriminates_candidates: true,
                    candidates_cover_disjoint_valid_facets: false,
                    same_or_versioned_evidence: false,
                    confidence: 0.9,
                    rationale: "A answers the query more directly.".to_owned(),
                }],
            }],
        };
        assert!(validate_review(&review, &[bundle()]).is_ok());
    }

    #[test]
    fn invalid_configuration_fails_closed() {
        assert!(validate_config(0, 14, 4, 0.7, 900, 4).is_err());
        assert!(validate_config(4, 15, 4, 0.7, 900, 4).is_err());
        assert!(validate_config(4, 14, 9, 0.7, 900, 4).is_err());
        assert!(validate_config(4, 14, 4, 0.49, 900, 4).is_err());
    }

    #[test]
    fn google_request_is_structured_and_contains_no_credential() {
        let request = google_request("review this", &response_schema(), 7);
        assert_eq!(
            request.pointer("/generationConfig/temperature"),
            Some(&json!(0.0))
        );
        assert_eq!(
            request.pointer("/generationConfig/responseMimeType"),
            Some(&json!("application/json"))
        );
        assert_eq!(
            request.pointer("/generationConfig/maxOutputTokens"),
            Some(&json!(MAX_OUTPUT_TOKENS))
        );
        assert!(request
            .pointer("/generationConfig/responseJsonSchema")
            .is_some());
        assert!(!request.to_string().contains("AIza"));
    }

    #[test]
    fn positive_evidence_gate_requires_direct_supported_evidence() {
        let valid = PositiveEvidenceAdjudication {
            bundle_slot: 0,
            input_consistent_and_answerable: true,
            positive_fully_supports_information_need: true,
            positive_requires_unsupported_inference: false,
            query_entities_match_positive: true,
            all_requested_parts_supported: true,
            reference_component_count: 2,
            supported_reference_component_count: 2,
            confidence: 0.95,
            rationale: "The candidate states the answer directly.".to_owned(),
        };
        assert!(valid.admits(0.9, true));
        assert!(!PositiveEvidenceAdjudication {
            positive_requires_unsupported_inference: true,
            ..valid
        }
        .admits(0.9, true));

        let partial = PositiveEvidenceAdjudication {
            bundle_slot: 0,
            input_consistent_and_answerable: true,
            positive_fully_supports_information_need: true,
            positive_requires_unsupported_inference: false,
            query_entities_match_positive: true,
            all_requested_parts_supported: false,
            reference_component_count: 2,
            supported_reference_component_count: 1,
            confidence: 1.0,
            rationale: "Only one requested component is stated.".to_owned(),
        };
        assert!(!partial.admits(0.9, true));
        assert!(!PositiveEvidenceAdjudication {
            all_requested_parts_supported: true,
            reference_component_count: 0,
            supported_reference_component_count: 0,
            ..partial
        }
        .admits(0.9, true));
    }

    #[test]
    fn curation_model_is_pinned_to_the_verified_free_tier_model() {
        assert!(validate_model("gemini-3.5-flash-lite").is_ok());
        assert!(validate_model("gemini-3.5-flash").is_err());
        assert_eq!(
            reviewer_identity("gemini-3.5-flash-lite"),
            "google-ai-studio-gemini-3.5-flash-lite-qps-v3-semantic-v8"
        );
    }

    #[test]
    fn grade_guard_requires_consistent_direction_and_gap() {
        let forward = Adjudication {
            challenger_slot: 0,
            verdict: CandidateVerdict::CandidateAPreferred,
            candidate_a_relevance: 4,
            candidate_b_relevance: 2,
            candidate_a_fully_supports_information_need: true,
            candidate_b_fully_supports_information_need: false,
            candidate_a_requires_unsupported_inference: false,
            candidate_b_requires_unsupported_inference: false,
            input_consistent_and_answerable: true,
            query_discriminates_candidates: true,
            candidates_cover_disjoint_valid_facets: false,
            same_or_versioned_evidence: false,
            confidence: 0.9,
            rationale: "A is more relevant".to_owned(),
        };
        let reverse = Adjudication {
            challenger_slot: 0,
            verdict: CandidateVerdict::CandidateBPreferred,
            candidate_a_relevance: 2,
            candidate_b_relevance: 4,
            candidate_a_fully_supports_information_need: false,
            candidate_b_fully_supports_information_need: true,
            candidate_a_requires_unsupported_inference: false,
            candidate_b_requires_unsupported_inference: false,
            input_consistent_and_answerable: true,
            query_discriminates_candidates: true,
            candidates_cover_disjoint_valid_facets: false,
            same_or_versioned_evidence: false,
            confidence: 0.9,
            rationale: "B is more relevant".to_owned(),
        };
        assert!(grades_support_verdict(
            &forward,
            &reverse,
            PairVerdict::PositivePreferred
        ));
        let tied = Adjudication {
            candidate_a_relevance: 4,
            candidate_b_relevance: 4,
            ..reverse
        };
        assert!(!grades_support_verdict(
            &forward,
            &tied,
            PairVerdict::PositivePreferred
        ));
    }

    #[test]
    fn short_underspecified_queries_require_an_irrelevant_loser() {
        let mut generic = bundle();
        generic.query = "Long Island".to_owned();
        generic.reference_answer.clear();
        let forward = Adjudication {
            challenger_slot: 0,
            verdict: CandidateVerdict::CandidateAPreferred,
            candidate_a_relevance: 4,
            candidate_b_relevance: 3,
            candidate_a_fully_supports_information_need: true,
            candidate_b_fully_supports_information_need: true,
            candidate_a_requires_unsupported_inference: false,
            candidate_b_requires_unsupported_inference: false,
            input_consistent_and_answerable: true,
            query_discriminates_candidates: true,
            candidates_cover_disjoint_valid_facets: false,
            same_or_versioned_evidence: false,
            confidence: 0.9,
            rationale: "A is more central".to_owned(),
        };
        let reverse = Adjudication {
            challenger_slot: 0,
            verdict: CandidateVerdict::CandidateBPreferred,
            candidate_a_relevance: 3,
            candidate_b_relevance: 4,
            candidate_a_fully_supports_information_need: true,
            candidate_b_fully_supports_information_need: true,
            candidate_a_requires_unsupported_inference: false,
            candidate_b_requires_unsupported_inference: false,
            input_consistent_and_answerable: true,
            query_discriminates_candidates: true,
            candidates_cover_disjoint_valid_facets: false,
            same_or_versioned_evidence: false,
            confidence: 0.9,
            rationale: "B is more central".to_owned(),
        };
        assert!(!semantic_guards_allow(
            &generic,
            &forward,
            &reverse,
            PairVerdict::PositivePreferred
        ));
        let clearly_irrelevant_forward = Adjudication {
            candidate_b_relevance: 0,
            ..forward
        };
        let clearly_irrelevant_reverse = Adjudication {
            candidate_a_relevance: 0,
            ..reverse
        };
        assert!(semantic_guards_allow(
            &generic,
            &clearly_irrelevant_forward,
            &clearly_irrelevant_reverse,
            PairVerdict::PositivePreferred
        ));

        let unsupported = Adjudication {
            candidate_a_requires_unsupported_inference: true,
            ..clearly_irrelevant_forward
        };
        assert!(!semantic_guards_allow(
            &generic,
            &unsupported,
            &clearly_irrelevant_reverse,
            PairVerdict::PositivePreferred
        ));
    }
}
