use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};
use serde::Deserialize;

#[derive(Clone, Debug)]
pub(crate) struct SourceDocument {
    pub id: String,
    pub title: String,
    pub text: String,
    pub source_family: String,
    pub collected_at: u64,
    pub source_time_label: String,
    pub reviewer_context: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceQuery {
    pub id: String,
    pub text: String,
    pub reference_answer: String,
    pub relevant: HashMap<String, u8>,
    pub family: String,
    pub entity_family: String,
    pub collection_cohort: String,
    pub collected_at: u64,
    pub kind: QueryKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueryKind {
    ScientificClaim,
    Conversation { category: u8 },
    MedicalInformation,
}

#[derive(Debug)]
pub(crate) struct SourceDataset {
    pub name: &'static str,
    pub documents: Vec<SourceDocument>,
    pub queries: Vec<SourceQuery>,
}

pub(crate) fn near_duplicate_key(document: &SourceDocument) -> String {
    let normalized = document
        .text
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.len() >= 40 && normalized.split_whitespace().count() >= 8 {
        format!("content:{normalized}")
    } else {
        format!("low-information-document:{}", document.id)
    }
}

pub(crate) fn load_beir(
    root: &Path,
    name: &'static str,
    qrels_split: &str,
    kind: QueryKind,
    collected_at: u64,
) -> Result<SourceDataset> {
    let documents = read_jsonl::<BeirDocument>(&root.join("corpus.jsonl"))?
        .into_iter()
        .map(|document| SourceDocument {
            source_family: format!("{name}:document:{}", document.id),
            id: document.id,
            title: document.title,
            text: document.text,
            collected_at,
            source_time_label: String::new(),
            reviewer_context: String::new(),
        })
        .collect::<Vec<_>>();
    let raw_queries = read_jsonl::<BeirQuery>(&root.join("queries.jsonl"))?;
    let qrels = read_qrels(&root.join("qrels").join(format!("{qrels_split}.tsv")))?;
    let queries = raw_queries
        .into_iter()
        .filter_map(|query| {
            let relevant = qrels.get(&query.id)?.clone();
            Some(SourceQuery {
                family: format!("{name}:query-family:{}", normalized_family(&query.text)),
                entity_family: format!("{name}:entity:{}", entity_family(&query.text)),
                collection_cohort: format!("{name}:{qrels_split}:annotation:{}", query.id),
                id: query.id,
                text: query.text,
                reference_answer: String::new(),
                relevant,
                collected_at,
                kind,
            })
        })
        .collect();
    Ok(SourceDataset {
        name,
        documents,
        queries,
    })
}

pub(crate) fn load_locomo(path: &Path, scenes: std::ops::Range<usize>) -> Result<SourceDataset> {
    let raw: Vec<LocomoSample> = serde_json::from_reader(BufReader::new(
        File::open(path).with_context(|| format!("open LoCoMo {}", path.display()))?,
    ))
    .with_context(|| format!("decode LoCoMo {}", path.display()))?;
    let selected = scenes.collect::<HashSet<_>>();
    let mut documents = Vec::new();
    let mut queries = Vec::new();
    let mut evidence_time = HashMap::<String, u64>::new();
    for (scene, sample) in raw.into_iter().enumerate() {
        if !selected.contains(&scene) {
            continue;
        }
        let mut known_evidence = HashSet::new();
        for session in sample.conversation.sessions() {
            let timestamp = locomo_timestamp(scene, session.number);
            for turn in session.turns {
                evidence_time.insert(turn.dia_id.clone(), timestamp);
                known_evidence.insert(turn.dia_id.clone());
                let reviewer_context = turn.reviewer_context();
                documents.push(SourceDocument {
                    id: format!("{}:{}", sample.sample_id, turn.dia_id),
                    title: turn.speaker,
                    text: turn.text,
                    source_family: format!("locomo:{}:conversation", sample.sample_id),
                    collected_at: timestamp,
                    source_time_label: session.time_label.clone(),
                    reviewer_context,
                });
            }
        }
        for (qa_index, qa) in sample.qa.into_iter().enumerate() {
            let relevant = qa
                .evidence
                .iter()
                .filter(|id| known_evidence.contains(*id))
                .map(|id| (format!("{}:{id}", sample.sample_id), 4_u8))
                .collect::<HashMap<_, _>>();
            if relevant.is_empty() {
                continue;
            }
            let timestamp = qa
                .evidence
                .iter()
                .filter_map(|id| evidence_time.get(id).copied())
                .max()
                .unwrap_or_else(|| locomo_timestamp(scene, 1));
            let reference_answer = qa.reference_answer();
            queries.push(SourceQuery {
                id: format!("{}:qa:{qa_index}", sample.sample_id),
                family: format!(
                    "locomo:{}:query-family:{}",
                    sample.sample_id,
                    normalized_family(&qa.question)
                ),
                entity_family: format!(
                    "locomo:{}:entity:{}",
                    sample.sample_id,
                    entity_family(&qa.question)
                ),
                collection_cohort: format!("locomo:{}:evidence-day:{timestamp}", sample.sample_id),
                text: qa.question,
                reference_answer,
                relevant,
                collected_at: timestamp,
                kind: QueryKind::Conversation {
                    category: qa.category,
                },
            });
        }
    }
    remove_cross_source_duplicates(&mut documents, &mut queries);
    Ok(SourceDataset {
        name: "locomo",
        documents,
        queries,
    })
}

fn remove_cross_source_duplicates(
    documents: &mut Vec<SourceDocument>,
    queries: &mut Vec<SourceQuery>,
) -> usize {
    let mut first_source = HashMap::<String, String>::new();
    let mut contaminated = HashSet::<String>::new();
    for document in documents.iter() {
        let key = near_duplicate_key(document);
        match first_source.get(&key) {
            Some(source) if source != &document.source_family => {
                contaminated.insert(key);
            }
            None => {
                first_source.insert(key, document.source_family.clone());
            }
            _ => {}
        }
    }
    if contaminated.is_empty() {
        return 0;
    }
    let before = documents.len();
    documents.retain(|document| !contaminated.contains(&near_duplicate_key(document)));
    let retained = documents
        .iter()
        .map(|document| document.id.as_str())
        .collect::<HashSet<_>>();
    for query in queries.iter_mut() {
        query
            .relevant
            .retain(|document, _| retained.contains(document.as_str()));
    }
    queries.retain(|query| !query.relevant.is_empty());
    before - documents.len()
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    let reader =
        BufReader::new(File::open(path).with_context(|| format!("open JSONL {}", path.display()))?);
    reader
        .lines()
        .enumerate()
        .map(|(line, value)| {
            let value =
                value.with_context(|| format!("read {} line {}", path.display(), line + 1))?;
            serde_json::from_str(&value)
                .with_context(|| format!("decode {} line {}", path.display(), line + 1))
        })
        .collect()
}

fn read_qrels(path: &Path) -> Result<HashMap<String, HashMap<String, u8>>> {
    let reader =
        BufReader::new(File::open(path).with_context(|| format!("open qrels {}", path.display()))?);
    let mut result = HashMap::<String, HashMap<String, u8>>::new();
    for (line, raw) in reader.lines().enumerate() {
        let raw = raw.with_context(|| format!("read {} line {}", path.display(), line + 1))?;
        if line == 0 && raw.starts_with("query-id") {
            continue;
        }
        let mut columns = raw.split('\t');
        let query = columns.next().context("qrels query id missing")?;
        let document = columns.next().context("qrels document id missing")?;
        let score = columns
            .next()
            .context("qrels score missing")?
            .parse::<u8>()
            .context("qrels score is not u8")?;
        if columns.next().is_some() {
            bail!(
                "qrels {} line {} has extra columns",
                path.display(),
                line + 1
            );
        }
        if score > 0 {
            result
                .entry(query.to_owned())
                .or_default()
                .insert(document.to_owned(), score.min(4));
        }
    }
    Ok(result)
}

fn normalized_family(text: &str) -> String {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .filter(|token| !QUESTION_WORDS.contains(&token.as_str()))
        .take(8)
        .collect::<Vec<_>>()
        .join("-")
}

fn entity_family(text: &str) -> String {
    let tokens = text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.len() > 2)
        .map(str::to_ascii_lowercase)
        .filter(|token| !QUESTION_WORDS.contains(&token.as_str()))
        .take(3)
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        "unclassified".to_owned()
    } else {
        tokens.join("-")
    }
}

const QUESTION_WORDS: &[&str] = &[
    "a", "an", "and", "are", "did", "do", "does", "for", "from", "how", "in", "is", "of", "on",
    "or", "the", "to", "was", "were", "what", "when", "where", "which", "who", "why", "with",
];

fn locomo_timestamp(scene: usize, session: usize) -> u64 {
    // Session order is the benchmark's authoritative temporal sequence. The
    // fixed epoch preserves that order without pretending its free-form date
    // strings are normalized timestamps.
    1_680_000_000 + scene as u64 * 4_000_000 + session as u64 * 86_400
}

#[derive(Deserialize)]
struct BeirDocument {
    #[serde(rename = "_id")]
    id: String,
    #[serde(default)]
    title: String,
    text: String,
}

#[derive(Deserialize)]
struct BeirQuery {
    #[serde(rename = "_id")]
    id: String,
    text: String,
}

#[derive(Deserialize)]
struct LocomoSample {
    sample_id: String,
    conversation: LocomoConversation,
    qa: Vec<LocomoQa>,
}

#[derive(Deserialize)]
struct LocomoQa {
    question: String,
    #[serde(default)]
    answer: serde_json::Value,
    #[serde(default)]
    adversarial_answer: serde_json::Value,
    evidence: Vec<String>,
    category: u8,
}

impl LocomoQa {
    fn reference_answer(&self) -> String {
        let value = if self.answer.is_null() {
            self.adversarial_answer.clone()
        } else {
            self.answer.clone()
        };
        render_reference_answer(value)
    }
}

fn render_reference_answer(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value,
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Null => String::new(),
        value => serde_json::to_string(&value).unwrap_or_default(),
    }
}

#[derive(Deserialize)]
struct LocomoConversation {
    #[serde(flatten)]
    values: serde_json::Map<String, serde_json::Value>,
}

struct LocomoSession {
    number: usize,
    time_label: String,
    turns: Vec<LocomoTurn>,
}

impl LocomoConversation {
    fn sessions(self) -> Vec<LocomoSession> {
        let mut labels = HashMap::<usize, String>::new();
        let mut sessions = Vec::new();
        for (name, value) in self.values {
            let Some(suffix) = name.strip_prefix("session_") else {
                continue;
            };
            if let Some(number) = suffix
                .strip_suffix("_date_time")
                .and_then(|value| value.parse::<usize>().ok())
            {
                if let Some(label) = value.as_str() {
                    labels.insert(number, label.to_owned());
                }
                continue;
            }
            let Some(number) = suffix.parse::<usize>().ok() else {
                continue;
            };
            if let Ok(turns) = serde_json::from_value::<Vec<LocomoTurn>>(value) {
                sessions.push(LocomoSession {
                    number,
                    time_label: String::new(),
                    turns,
                });
            }
        }
        for session in &mut sessions {
            session.time_label = labels.remove(&session.number).unwrap_or_default();
        }
        sessions.sort_unstable_by_key(|session| session.number);
        sessions
    }
}

#[derive(Deserialize)]
struct LocomoTurn {
    speaker: String,
    dia_id: String,
    text: String,
    #[serde(default)]
    blip_caption: String,
    #[serde(default)]
    query: String,
}

impl LocomoTurn {
    fn reviewer_context(&self) -> String {
        match (self.blip_caption.is_empty(), self.query.is_empty()) {
            (true, true) => String::new(),
            (false, true) => format!("image caption: {}", self.blip_caption),
            (true, false) => format!("image query: {}", self.query),
            (false, false) => format!(
                "image caption: {}; image query: {}",
                self.blip_caption, self.query
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_normalization_removes_question_scaffolding() {
        assert_eq!(
            normalized_family("When did Caroline go to the support group?"),
            "caroline-go-support-group"
        );
    }

    #[test]
    fn session_order_becomes_strict_time_order() {
        assert!(locomo_timestamp(2, 8) > locomo_timestamp(2, 7));
        assert!(locomo_timestamp(3, 1) > locomo_timestamp(2, 1));
    }

    #[test]
    fn session_preserves_official_time_label_for_semantic_review() {
        let conversation: LocomoConversation = serde_json::from_value(serde_json::json!({
            "speaker_a": "A",
            "session_1_date_time": "1:56 pm on 8 May, 2023",
            "session_1": [{"speaker": "A", "dia_id": "D1:1", "text": "hello"}]
        }))
        .unwrap();
        let sessions = conversation.sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].time_label, "1:56 pm on 8 May, 2023");
    }

    #[test]
    fn reference_answer_accepts_numeric_and_adversarial_forms() {
        let numeric: LocomoQa = serde_json::from_value(serde_json::json!({
            "question": "when",
            "answer": 2022,
            "evidence": ["D1:1"],
            "category": 1
        }))
        .unwrap();
        let adversarial: LocomoQa = serde_json::from_value(serde_json::json!({
            "question": "why",
            "adversarial_answer": "because",
            "evidence": ["D1:1"],
            "category": 1
        }))
        .unwrap();
        assert_eq!(numeric.reference_answer(), "2022");
        assert_eq!(adversarial.reference_answer(), "because");
    }

    #[test]
    fn image_caption_is_preserved_only_as_reviewer_context() {
        let turn: LocomoTurn = serde_json::from_value(serde_json::json!({
            "speaker": "Melanie",
            "dia_id": "D1:12",
            "text": "take a look",
            "blip_caption": "a painting of a sunset over a lake",
            "query": "painting sunrise"
        }))
        .unwrap();
        assert_eq!(
            turn.reviewer_context(),
            "image caption: a painting of a sunset over a lake; image query: painting sunrise"
        );
    }

    #[test]
    fn low_information_turns_do_not_form_cross_source_duplicate_clusters() {
        let mut first = document("one", "Yes.");
        let second = document("two", "Yes.");
        assert_ne!(near_duplicate_key(&first), near_duplicate_key(&second));
        first.text = "This sufficiently long repeated document has enough distinct words to identify an actual duplicate cluster.".to_owned();
        let mut duplicate = first.clone();
        duplicate.id = "two".to_owned();
        assert_eq!(near_duplicate_key(&first), near_duplicate_key(&duplicate));
    }

    #[test]
    fn cross_source_duplicates_and_orphaned_queries_are_removed() {
        let repeated = "This sufficiently long repeated document has enough distinct words to identify an actual duplicate cluster.";
        let mut documents = vec![
            document("one", repeated),
            document("two", repeated),
            document(
                "unique",
                "This is a different sufficiently long document with distinct evidence for a retained gold query.",
            ),
        ];
        documents[0].source_family = "source-one".to_owned();
        documents[1].source_family = "source-two".to_owned();
        documents[2].source_family = "source-two".to_owned();
        let mut queries = vec![query("removed", "one"), query("retained", "unique")];
        assert_eq!(
            remove_cross_source_duplicates(&mut documents, &mut queries),
            2
        );
        assert_eq!(documents.len(), 1);
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].id, "retained");
    }

    fn document(id: &str, text: &str) -> SourceDocument {
        SourceDocument {
            id: id.to_owned(),
            title: String::new(),
            text: text.to_owned(),
            source_family: id.to_owned(),
            collected_at: 1,
            source_time_label: String::new(),
            reviewer_context: String::new(),
        }
    }

    fn query(id: &str, relevant: &str) -> SourceQuery {
        SourceQuery {
            id: id.to_owned(),
            text: id.to_owned(),
            reference_answer: String::new(),
            relevant: [(relevant.to_owned(), 4)].into_iter().collect(),
            family: id.to_owned(),
            entity_family: id.to_owned(),
            collection_cohort: id.to_owned(),
            collected_at: 1,
            kind: QueryKind::ScientificClaim,
        }
    }
}
