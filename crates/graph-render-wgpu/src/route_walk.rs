//! Guided route walk: a frozen, ordered route that is traversed step by step.
//!
//! The route is captured once, as stable node and edge ids, when the walk
//! starts. Hover, incidental selection, and camera movement never reroute it.
//! Every availability problem has an explicit, deterministic outcome; the
//! walk never substitutes another path.

use graph_model::{EdgeId, NodeId};

/// Seconds for particles to carry one traversal from node to node.
pub(crate) const TRAVERSE_SECS: f32 = 1.2;
/// Seconds the walk rests on an arrived node before continuing to play.
const DWELL_SECS: f32 = 0.55;
/// Seconds for an arrival bloom to settle into the current-node glow.
const BLOOM_SECS: f32 = 0.9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteWalkCommand {
    Play,
    Pause,
    Next,
    Previous,
}

/// Why the walk is not simply playing. `step` is a route node index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteWalkNotice {
    /// Starting requires a selected node and a second route endpoint.
    NeedsEndpoints,
    /// The endpoints are not connected in the visible graph.
    NoPath,
    /// The route exceeds the bounded walk; it was not started.
    OverBound { limit: usize },
    /// The walk reached the final node.
    Arrived,
    /// A route member is hidden by the current view; the route is preserved.
    Unavailable { step: usize },
    /// A route member no longer exists; playback paused at the last valid step.
    Broken { step: usize },
    /// The scene changed and the frozen route could not be resolved in it.
    SceneChanged,
    /// No visible document has connections for the flow to walk.
    NoDocumentFlow,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RouteWalkPlayback {
    #[default]
    Paused,
    Playing,
    /// Completing one Next/Previous traversal, then pausing.
    Stepping,
}

/// Summary of the document flow for the shell strip.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlowStatus {
    pub documents: usize,
    pub connections: usize,
    pub levels: u32,
    pub settled: bool,
}

/// Snapshot the shell renders. `revision` changes only when a user-visible
/// field changes, never per animation frame.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RouteWalkStatus {
    pub revision: u64,
    pub active: bool,
    pub playback: RouteWalkPlayback,
    pub position: usize,
    pub node_count: usize,
    pub traversing_to: Option<usize>,
    pub notice: Option<RouteWalkNotice>,
    pub endpoints: Option<(NodeId, NodeId)>,
    /// Present while the document flow is running or settled.
    pub flow: Option<FlowStatus>,
    /// Present while the story timeline (4C) is open.
    pub story: Option<crate::StoryStatus>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Membership {
    Present,
    Hidden,
    Missing,
}

/// Resolves frozen ids against the live scene without rerouting.
pub(crate) trait RouteAvailability {
    fn node(&self, id: NodeId) -> Membership;
    fn edge(&self, id: EdgeId, from: NodeId, to: NodeId) -> Membership;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Traversal {
    pub from: usize,
    pub to: usize,
    pub progress: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct RouteWalk {
    nodes: Vec<NodeId>,
    edges: Vec<EdgeId>,
    current: usize,
    traversal: Option<Traversal>,
    playback: RouteWalkPlayback,
    dwell: f32,
    bloom: f32,
    notice: Option<RouteWalkNotice>,
}

impl RouteWalk {
    /// `edges[i]` joins `nodes[i]` and `nodes[i + 1]`.
    pub(crate) fn new(nodes: Vec<NodeId>, edges: Vec<EdgeId>) -> Option<Self> {
        if nodes.len() < 2 || edges.len() + 1 != nodes.len() {
            return None;
        }
        Some(Self {
            nodes,
            edges,
            current: 0,
            traversal: None,
            playback: RouteWalkPlayback::Paused,
            dwell: 0.0,
            bloom: 1.0,
            notice: None,
        })
    }

    /// Begins continuous playback after a short reveal of the constellation.
    pub(crate) fn start_playing(&mut self) {
        self.playback = RouteWalkPlayback::Playing;
        self.dwell = 1.0;
        self.bloom = 1.0;
    }

    pub(crate) fn nodes(&self) -> &[NodeId] {
        &self.nodes
    }

    pub(crate) fn current(&self) -> usize {
        self.current
    }

    pub(crate) fn traversal(&self) -> Option<Traversal> {
        self.traversal
    }

    pub(crate) fn bloom(&self) -> f32 {
        self.bloom
    }

    pub(crate) fn playback(&self) -> RouteWalkPlayback {
        self.playback
    }

    pub(crate) fn notice(&self) -> Option<RouteWalkNotice> {
        self.notice
    }

    fn last(&self) -> usize {
        self.nodes.len() - 1
    }

    /// True while frames must be produced. A paused walk is fully static.
    pub(crate) fn animating(&self) -> bool {
        self.playback != RouteWalkPlayback::Paused
    }

    pub(crate) fn edge_between(&self, a: usize, b: usize) -> Option<EdgeId> {
        (a.abs_diff(b) == 1).then(|| self.edges[a.min(b)])
    }

    pub(crate) fn command(&mut self, command: RouteWalkCommand, scene: &impl RouteAvailability) {
        match command {
            RouteWalkCommand::Play => {
                if self.traversal.is_none() && self.current == self.last() {
                    self.current = 0;
                    self.bloom = 1.0;
                    self.dwell = DWELL_SECS;
                }
                self.notice = None;
                self.playback = RouteWalkPlayback::Playing;
                if self.traversal.is_none() && !self.check_node(self.current, scene) {
                    self.playback = RouteWalkPlayback::Paused;
                }
            }
            RouteWalkCommand::Pause => self.playback = RouteWalkPlayback::Paused,
            RouteWalkCommand::Next => {
                if self.traversal.is_some() {
                    self.playback = RouteWalkPlayback::Stepping;
                } else if self.current < self.last() {
                    self.notice = None;
                    if self.begin(self.current + 1, scene) {
                        self.playback = RouteWalkPlayback::Stepping;
                    }
                } else {
                    self.notice = Some(RouteWalkNotice::Arrived);
                }
            }
            RouteWalkCommand::Previous => {
                if let Some(traversal) = self.traversal {
                    self.traversal = Some(Traversal {
                        from: traversal.to,
                        to: traversal.from,
                        progress: 1.0 - traversal.progress,
                    });
                    self.notice = None;
                    self.playback = RouteWalkPlayback::Stepping;
                } else if self.current > 0 {
                    self.notice = None;
                    if self.begin(self.current - 1, scene) {
                        self.playback = RouteWalkPlayback::Stepping;
                    }
                }
            }
        }
    }

    /// Advances the animation clock. Returns true when the current step
    /// changed, so callers can refresh non-animated state.
    pub(crate) fn advance(&mut self, elapsed: f32, scene: &impl RouteAvailability) -> bool {
        if self.playback == RouteWalkPlayback::Paused {
            return false;
        }
        let dt = elapsed.clamp(0.0, 0.1);
        if let Some(mut traversal) = self.traversal {
            traversal.progress += dt / TRAVERSE_SECS;
            if traversal.progress < 1.0 {
                self.traversal = Some(traversal);
                return false;
            }
            self.traversal = None;
            self.current = traversal.to;
            self.bloom = 1.0;
            self.dwell = DWELL_SECS;
            if self.current == self.last() && self.playback == RouteWalkPlayback::Playing {
                self.notice = Some(RouteWalkNotice::Arrived);
            }
            return true;
        }
        self.bloom = (self.bloom - dt / BLOOM_SECS).max(0.0);
        match self.playback {
            RouteWalkPlayback::Playing if self.current < self.last() => {
                self.dwell -= dt;
                if self.dwell <= 0.0 && !self.begin(self.current + 1, scene) {
                    self.playback = RouteWalkPlayback::Paused;
                }
            }
            RouteWalkPlayback::Playing | RouteWalkPlayback::Stepping if self.bloom <= 0.0 => {
                self.playback = RouteWalkPlayback::Paused;
            }
            _ => {}
        }
        false
    }

    /// Re-checks the frozen route against the live scene. A missing member
    /// pauses at the last valid step; hidden members are reported but the
    /// route is kept. Returns false when the route cannot be resolved at all.
    pub(crate) fn revalidate(&mut self, scene: &impl RouteAvailability) -> bool {
        for (step, &node) in self.nodes.iter().enumerate() {
            if scene.node(node) == Membership::Missing {
                self.break_at(step);
                return false;
            }
        }
        for (index, &edge) in self.edges.iter().enumerate() {
            if scene.edge(edge, self.nodes[index], self.nodes[index + 1]) == Membership::Missing {
                self.break_at(index + 1);
                return false;
            }
        }
        true
    }

    fn break_at(&mut self, step: usize) {
        self.traversal = None;
        self.playback = RouteWalkPlayback::Paused;
        self.notice = Some(RouteWalkNotice::Broken { step });
        if step <= self.current {
            self.current = step.saturating_sub(1);
        }
    }

    fn check_node(&mut self, step: usize, scene: &impl RouteAvailability) -> bool {
        match scene.node(self.nodes[step]) {
            Membership::Present => true,
            Membership::Hidden => {
                self.notice = Some(RouteWalkNotice::Unavailable { step });
                false
            }
            Membership::Missing => {
                self.notice = Some(RouteWalkNotice::Broken { step });
                false
            }
        }
    }

    fn begin(&mut self, to: usize, scene: &impl RouteAvailability) -> bool {
        let from = self.current;
        let edge = self.edges[from.min(to)];
        let membership = match scene.node(self.nodes[to]) {
            Membership::Present => scene.edge(edge, self.nodes[from], self.nodes[to]),
            other => other,
        };
        match membership {
            Membership::Present => {
                self.traversal = Some(Traversal {
                    from,
                    to,
                    progress: 0.0,
                });
                true
            }
            Membership::Hidden => {
                self.notice = Some(RouteWalkNotice::Unavailable { step: to });
                self.playback = RouteWalkPlayback::Paused;
                false
            }
            Membership::Missing => {
                self.notice = Some(RouteWalkNotice::Broken { step: to });
                self.playback = RouteWalkPlayback::Paused;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Scene {
        nodes: HashMap<u64, Membership>,
        edges: HashMap<u64, Membership>,
    }

    impl RouteAvailability for Scene {
        fn node(&self, id: NodeId) -> Membership {
            self.nodes.get(&id.0).copied().unwrap_or(Membership::Present)
        }
        fn edge(&self, id: EdgeId, _: NodeId, _: NodeId) -> Membership {
            self.edges.get(&id.0).copied().unwrap_or(Membership::Present)
        }
    }

    fn walk() -> RouteWalk {
        RouteWalk::new(
            vec![NodeId(1), NodeId(2), NodeId(3), NodeId(4)],
            vec![EdgeId(12), EdgeId(23), EdgeId(34)],
        )
        .expect("valid route")
    }

    fn run(walk: &mut RouteWalk, scene: &Scene, seconds: f32) {
        let mut left = seconds;
        while left > 0.0 {
            walk.advance(0.05, scene);
            left -= 0.05;
        }
    }

    #[test]
    fn next_traverses_one_edge_then_pauses_and_previous_reverses() {
        let scene = Scene::default();
        let mut walk = walk();
        walk.command(RouteWalkCommand::Next, &scene);
        assert_eq!(walk.traversal().map(|t| (t.from, t.to)), Some((0, 1)));
        run(&mut walk, &scene, TRAVERSE_SECS + BLOOM_SECS + 0.2);
        assert_eq!(walk.current(), 1);
        assert_eq!(walk.playback(), RouteWalkPlayback::Paused);
        walk.command(RouteWalkCommand::Previous, &scene);
        assert_eq!(walk.traversal().map(|t| (t.from, t.to)), Some((1, 0)));
        run(&mut walk, &scene, TRAVERSE_SECS + BLOOM_SECS + 0.2);
        assert_eq!(walk.current(), 0);
    }

    #[test]
    fn pause_freezes_the_exact_position_and_play_runs_to_arrival() {
        let scene = Scene::default();
        let mut walk = walk();
        walk.command(RouteWalkCommand::Play, &scene);
        run(&mut walk, &scene, DWELL_SECS + TRAVERSE_SECS * 0.5);
        walk.command(RouteWalkCommand::Pause, &scene);
        let frozen = walk.traversal();
        assert!(frozen.is_some());
        run(&mut walk, &scene, 5.0);
        assert_eq!(walk.traversal(), frozen);
        walk.command(RouteWalkCommand::Play, &scene);
        run(&mut walk, &scene, 20.0);
        assert_eq!(walk.current(), 3);
        assert_eq!(walk.notice(), Some(RouteWalkNotice::Arrived));
        assert_eq!(walk.playback(), RouteWalkPlayback::Paused);
    }

    #[test]
    fn hidden_member_pauses_without_rerouting_and_resumes_when_visible() {
        let mut scene = Scene::default();
        scene.nodes.insert(3, Membership::Hidden);
        let mut walk = walk();
        walk.command(RouteWalkCommand::Play, &scene);
        run(&mut walk, &scene, 10.0);
        assert_eq!(walk.current(), 1);
        assert_eq!(walk.notice(), Some(RouteWalkNotice::Unavailable { step: 2 }));
        assert_eq!(walk.nodes(), &[NodeId(1), NodeId(2), NodeId(3), NodeId(4)]);
        scene.nodes.clear();
        walk.command(RouteWalkCommand::Play, &scene);
        run(&mut walk, &scene, 20.0);
        assert_eq!(walk.current(), 3);
    }

    #[test]
    fn missing_member_breaks_at_the_last_valid_step() {
        let mut scene = Scene::default();
        let mut walk = walk();
        walk.command(RouteWalkCommand::Next, &scene);
        run(&mut walk, &scene, 3.0);
        walk.command(RouteWalkCommand::Next, &scene);
        run(&mut walk, &scene, 3.0);
        assert_eq!(walk.current(), 2);
        scene.edges.insert(23, Membership::Missing);
        assert!(!walk.revalidate(&scene));
        assert_eq!(walk.notice(), Some(RouteWalkNotice::Broken { step: 2 }));
        assert_eq!(walk.current(), 1);
        assert_eq!(walk.playback(), RouteWalkPlayback::Paused);
    }

    #[test]
    fn malformed_routes_are_rejected() {
        assert!(RouteWalk::new(vec![NodeId(1)], vec![]).is_none());
        assert!(RouteWalk::new(vec![NodeId(1), NodeId(2)], vec![]).is_none());
    }
}
