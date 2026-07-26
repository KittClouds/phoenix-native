use crate::{GraphEvent, RenderError};
use graph_model::NodeId;
use std::collections::VecDeque;

const MAX_PENDING_EVENTS: usize = 32;

pub(crate) struct PendingEvents {
    queue: VecDeque<GraphEvent>,
    selection_sequence: u64,
}

impl PendingEvents {
    pub(crate) fn new() -> Self {
        Self {
            queue: VecDeque::with_capacity(8),
            selection_sequence: 0,
        }
    }

    pub(crate) fn selection(&mut self, node: Option<NodeId>) -> Result<GraphEvent, RenderError> {
        self.selection_sequence = self
            .selection_sequence
            .checked_add(1)
            .ok_or(RenderError::SelectionSequenceExhausted)?;
        Ok(GraphEvent::SelectionChanged {
            sequence: self.selection_sequence,
            node,
        })
    }

    pub(crate) fn push_hover(&mut self, node: Option<NodeId>) {
        self.queue
            .retain(|event| !matches!(event, GraphEvent::HoverChanged(_)));
        self.push(GraphEvent::HoverChanged(node));
    }

    pub(crate) fn push_selection(&mut self, node: Option<NodeId>) -> Result<(), RenderError> {
        let event = self.selection(node)?;
        self.push(event);
        Ok(())
    }

    pub(crate) fn drain(&mut self) -> impl Iterator<Item = GraphEvent> + '_ {
        self.queue.drain(..)
    }

    fn push(&mut self, event: GraphEvent) {
        if self.queue.len() == MAX_PENDING_EVENTS {
            self.queue.pop_front();
        }
        self.queue.push_back(event);
    }
}

#[cfg(test)]
mod tests {
    use super::PendingEvents;
    use crate::GraphEvent;
    use graph_model::NodeId;

    #[test]
    fn hover_events_are_latest_wins() {
        let mut events = PendingEvents::new();
        events.push_hover(Some(NodeId(1)));
        events.push_hover(Some(NodeId(2)));
        assert_eq!(
            events.drain().collect::<Vec<_>>(),
            vec![GraphEvent::HoverChanged(Some(NodeId(2)))]
        );
    }

    #[test]
    fn selections_are_monotonically_sequenced() {
        let mut events = PendingEvents::new();
        events
            .push_selection(Some(NodeId(7)))
            .unwrap_or_else(|error| panic!("{error}"));
        events
            .push_selection(None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            events.drain().collect::<Vec<_>>(),
            vec![
                GraphEvent::SelectionChanged {
                    sequence: 1,
                    node: Some(NodeId(7)),
                },
                GraphEvent::SelectionChanged {
                    sequence: 2,
                    node: None,
                },
            ]
        );
    }
}
