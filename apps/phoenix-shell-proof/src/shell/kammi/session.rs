use serde::{Deserialize, Serialize};
use velotype::AgentAnchor;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KammiSession {
    pub id: u64,
    pub title: String,
    pub messages: Vec<KammiMessage>,
    pub created_at: u64,
}

impl KammiSession {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            title: "New Session".to_string(),
            messages: Vec::new(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }

    pub fn update_title_from_first_message(&mut self) {
        if let Some(first_user_msg) = self.messages.iter().find(|m| m.role == KammiRole::User) {
            let mut title = first_user_msg.content.trim().replace('\n', " ");
            if title.chars().count() > 48 {
                title = title.chars().take(48).collect::<String>() + "…";
            }
            if !title.is_empty() {
                self.title = title;
            }
        }
    }

    pub fn streaming_assistant_mut(&mut self, request_id: u64) -> Option<&mut KammiMessage> {
        self.messages.iter_mut().find(|message| {
            message.id == request_id
                && message.role == KammiRole::Assistant
                && message.state == MessageState::Streaming
        })
    }

    pub fn interrupt_orphaned_streams(&mut self) {
        for message in &mut self.messages {
            if message.state == MessageState::Streaming {
                message.state = MessageState::Interrupted;
            }
        }
    }

    pub fn max_identity(&self) -> u64 {
        self.messages
            .iter()
            .map(|message| message.id)
            .fold(self.id, u64::max)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KammiMessage {
    pub id: u64,
    pub role: KammiRole,
    pub content: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub state: MessageState,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum KammiRole {
    User,
    Assistant,
    System,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageState {
    #[default]
    Complete,
    Streaming,
    Interrupted,
    Failed,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PendingInsertion {
    pub request_id: u64,
    pub anchor: AgentAnchor,
    pub context_digest: [u8; 32],
}

pub fn digest_messages(messages: &[KammiMessage]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    for message in messages {
        let role: &[u8] = match message.role {
            KammiRole::System => b"system",
            KammiRole::User => b"user",
            KammiRole::Assistant => b"assistant",
        };
        hasher.update(role);
        hasher.update(&[0]);
        hasher.update(message.content.as_bytes());
        hasher.update(&[0xff]);
    }
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: u64, state: MessageState) -> KammiMessage {
        KammiMessage {
            id,
            role: KammiRole::Assistant,
            content: String::new(),
            model: None,
            state,
        }
    }

    #[test]
    fn streaming_updates_are_bound_to_the_request_identity() {
        let mut session = KammiSession::new(1);
        session.messages = vec![
            message(10, MessageState::Streaming),
            message(11, MessageState::Streaming),
        ];

        assert_eq!(
            session
                .streaming_assistant_mut(10)
                .map(|message| message.id),
            Some(10)
        );
        assert!(session.streaming_assistant_mut(12).is_none());
    }

    #[test]
    fn restart_marks_unowned_streams_interrupted_and_recovers_max_id() {
        let mut session = KammiSession::new(7);
        session.messages = vec![message(41, MessageState::Streaming)];
        session.interrupt_orphaned_streams();

        assert_eq!(session.messages[0].state, MessageState::Interrupted);
        assert_eq!(session.max_identity(), 41);
    }
}
