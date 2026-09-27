//! Guided route walk presentation for [`GraphRenderer`].
//!
//! The walk owns a frozen route of stable ids. Rendering state is written
//! only into the display overlay word and the particle layer, so exiting
//! restores the exact pre-walk atlas without touching graph data, camera, or
//! selection.

use super::GraphRenderer;
use crate::interaction_index::{RouteOutcome, MAX_ROUTE_EDGES};
use crate::particles::{build_particles, ParticleCurve, ParticleFrame};
use crate::route_walk::{Membership, RouteAvailability, RouteWalk};
use crate::{
    RouteWalkCommand, RouteWalkNotice, RouteWalkStatus, WALK_CURRENT, WALK_GLOW_SHIFT, WALK_NEXT,
    WALK_ROUTE, WALK_VISITED,
};
use graph_model::{EdgeId, NodeId};

pub(crate) struct ActiveWalk {
    route: RouteWalk,
    /// Cached traversal curve keyed by (edge id, from node id).
    curve: Option<((EdgeId, NodeId), ParticleCurve)>,
}

struct SceneAvailability<'a>(&'a crate::gpu_scene::GpuScene);

impl RouteAvailability for SceneAvailability<'_> {
    fn node(&self, id: NodeId) -> Membership {
        match self.0.state().node_slot(id) {
            None => Membership::Missing,
            Some(slot) if self.0.node_active(slot) => Membership::Present,
            Some(_) => Membership::Hidden,
        }
    }

    fn edge(&self, id: EdgeId, from: NodeId, to: NodeId) -> Membership {
        let Some(slot) = self.0.state().edge_slot(id) else {
            return Membership::Missing;
        };
        let Some(edge) = self.0.state().edge_at_slot(slot) else {
            return Membership::Missing;
        };
        let joins = (edge.source == from && edge.target == to)
            || (edge.source == to && edge.target == from);
        if !joins {
            Membership::Missing
        } else if self.0.edge_active(slot) {
            Membership::Present
        } else {
            Membership::Hidden
        }
    }
}

impl GraphRenderer {
    /// Freezes the bounded route between the selected node and the second
    /// endpoint, then begins playing it. Hover never supplies an endpoint.
    pub fn start_route_walk(&mut self) -> &RouteWalkStatus {
        self.end_document_flow();
        self.end_route_walk(None);
        let endpoints = self
            .scene
            .selected_node()
            .zip(self.scene.secondary_selected_node());
        let Some((source_id, target_id)) = endpoints else {
            return self.publish_walk_status(Some(RouteWalkNotice::NeedsEndpoints));
        };
        let slots = self
            .scene
            .state()
            .node_slot(source_id)
            .zip(self.scene.state().node_slot(target_id))
            .filter(|(source, target)| {
                self.scene.node_active(*source) && self.scene.node_active(*target)
            });
        let Some((source, target)) = slots else {
            return self.publish_walk_status(Some(RouteWalkNotice::NoPath));
        };
        let (outcome, node_slots, edge_slots) = self.scene.frozen_route(source, target);
        match outcome {
            RouteOutcome::NoPath => {
                return self.publish_walk_status(Some(RouteWalkNotice::NoPath));
            }
            RouteOutcome::OverBound => {
                return self.publish_walk_status(Some(RouteWalkNotice::OverBound {
                    limit: MAX_ROUTE_EDGES,
                }));
            }
            RouteOutcome::Found => {}
        }
        let state = self.scene.state();
        let nodes = node_slots
            .iter()
            .filter_map(|&slot| state.node_at_slot(slot).map(|node| node.id))
            .collect::<Vec<_>>();
        let edges = edge_slots
            .iter()
            .filter_map(|&slot| state.edge_at_slot(slot).map(|edge| edge.id))
            .collect::<Vec<_>>();
        let Some(mut route) = RouteWalk::new(nodes, edges) else {
            return self.publish_walk_status(Some(RouteWalkNotice::NoPath));
        };
        route.start_playing();
        self.walk = Some(ActiveWalk { route, curve: None });
        self.refresh_walk_presentation(true);
        self.publish_walk_status(None)
    }

    pub fn route_walk_command(&mut self, command: RouteWalkCommand) -> &RouteWalkStatus {
        if let Some(mut walk) = self.walk.take() {
            walk.route
                .command(command, &SceneAvailability(&self.scene));
            self.walk = Some(walk);
            self.refresh_walk_presentation(false);
        }
        let notice = self.walk.as_ref().and_then(|walk| walk.route.notice());
        self.publish_walk_status(notice)
    }

    /// Dissolves the walk treatment. Selection, camera, filters, and graph
    /// data were never changed, so this restores the pre-walk atlas exactly.
    pub fn exit_route_walk(&mut self) -> &RouteWalkStatus {
        self.end_route_walk(None);
        self.publish_walk_status(None)
    }

    #[must_use]
    pub fn route_walk_status(&self) -> &RouteWalkStatus {
        &self.walk_status
    }

    pub(super) fn walk_animating(&self) -> bool {
        self.walk
            .as_ref()
            .is_some_and(|walk| walk.route.animating())
    }

    pub(super) fn advance_route_walk(&mut self, elapsed: f32) {
        let Some(mut walk) = self.walk.take() else {
            return;
        };
        if !walk.route.animating() {
            self.walk = Some(walk);
            return;
        }
        walk.route.advance(elapsed, &SceneAvailability(&self.scene));
        self.walk = Some(walk);
        self.refresh_walk_presentation(false);
        let notice = self.walk.as_ref().and_then(|walk| walk.route.notice());
        self.publish_walk_status(notice);
    }

    /// Re-resolves the frozen route after the resident scene changed. A new
    /// scene keeps the walk only when every member resolves identically;
    /// an in-place diff pauses at the last valid step instead.
    pub(super) fn revalidate_route_walk(&mut self, scene_replaced: bool) {
        let Some(mut walk) = self.walk.take() else {
            return;
        };
        walk.curve = None;
        let resolved = walk.route.revalidate(&SceneAvailability(&self.scene));
        if !resolved && scene_replaced {
            self.end_route_walk(Some(RouteWalkNotice::SceneChanged));
            self.publish_walk_status(Some(RouteWalkNotice::SceneChanged));
            return;
        }
        self.walk = Some(walk);
        self.refresh_walk_presentation(true);
        let notice = self.walk.as_ref().and_then(|walk| walk.route.notice());
        self.publish_walk_status(notice);
    }

    pub(super) fn invalidate_walk_curve(&mut self) {
        if let Some(walk) = self.walk.as_mut() {
            walk.curve = None;
        }
    }

    pub(super) fn end_route_walk(&mut self, _reason: Option<RouteWalkNotice>) {
        // A walk and the story both own node overlays; the walk wins.
        self.end_story();
        if self.walk.take().is_some() {
            self.scene.set_walk_overlay(false, Vec::new(), &self.queue);
            self.particles.clear();
            self.labels.mark_dirty();
            self.redraw_requested = true;
        }
    }

    pub(super) fn publish_walk_status(
        &mut self,
        notice: Option<RouteWalkNotice>,
    ) -> &RouteWalkStatus {
        let mut next = RouteWalkStatus {
            revision: self.walk_status.revision,
            notice,
            ..RouteWalkStatus::default()
        };
        if let Some(walk) = self.walk.as_ref() {
            let nodes = walk.route.nodes();
            next.active = true;
            next.playback = walk.route.playback();
            next.position = walk.route.current();
            next.node_count = nodes.len();
            next.traversing_to = walk.route.traversal().map(|traversal| traversal.to);
            next.endpoints = nodes.first().copied().zip(nodes.last().copied());
        }
        next.flow = self.flow.as_ref().map(super::flow::ActiveFlow::status);
        next.story = self.story_status();
        if next != self.walk_status {
            next.revision = self.walk_status.revision.wrapping_add(1);
            self.walk_status = next;
        }
        &self.walk_status
    }

    /// Writes overlay bits for route members and rebuilds the particle
    /// stream. `full` restamps every node (start, exit, scene change).
    fn refresh_walk_presentation(&mut self, full: bool) {
        let Some(walk) = self.walk.as_mut() else {
            return;
        };
        let route = &walk.route;
        let state = self.scene.state();
        let current = route.current();
        let traversal = route.traversal();
        let mut entries = Vec::with_capacity(route.nodes().len());
        for (index, &id) in route.nodes().iter().enumerate() {
            let Some(slot) = state.node_slot(id) else {
                continue;
            };
            let mut bits = if index == current {
                WALK_CURRENT | glow_bits(route.bloom().max(0.35))
            } else if index < current {
                WALK_VISITED
            } else {
                WALK_ROUTE
            };
            if let Some(traversal) = traversal.filter(|traversal| traversal.to == index) {
                let anticipation = smoothstep(0.72, 1.0, traversal.progress);
                bits = WALK_NEXT | glow_bits(anticipation);
                if index < current {
                    bits |= WALK_VISITED;
                }
            }
            entries.push((slot, bits));
        }

        // Particle curve for the traversal in flight, oriented from -> to.
        let mut frame_curve = None;
        let mut colors = ([1.0; 4], [1.0; 4]);
        let mut progress = 0.0;
        if let Some(traversal) = traversal {
            let from = route.nodes()[traversal.from];
            let to = route.nodes()[traversal.to];
            let edge = route
                .edge_between(traversal.from, traversal.to)
                .expect("traversal joins adjacent route nodes");
            let key = (edge, from);
            if walk.curve.as_ref().is_none_or(|(cached, _)| *cached != key) {
                walk.curve = traversal_curve(&self.scene, &self.prepared_paths, edge, from, to)
                    .map(|curve| (key, curve));
            }
            frame_curve = walk.curve.as_ref().map(|(_, curve)| curve);
            let color = |id: NodeId| {
                state
                    .node_slot(id)
                    .and_then(|slot| self.scene.node_color(slot))
                    .unwrap_or([1.0; 4])
            };
            colors = (color(from), color(to));
            progress = traversal.progress;
        }
        let bloom = (traversal.is_none() && route.bloom() > 0.0)
            .then(|| {
                let id = route.nodes()[current];
                let slot = state.node_slot(id)?;
                let node = state.node_at_slot(slot)?;
                Some((
                    node.position,
                    self.scene.node_color(slot).unwrap_or([1.0; 4]),
                    route.bloom(),
                ))
            })
            .flatten();
        let camera = &self.camera;
        let project = |position: [f32; 3]| {
            camera
                .project_to_viewport(position)
                .map(|(x, y, _)| (x, y))
        };
        build_particles(
            &ParticleFrame {
                curve: frame_curve,
                progress,
                from_color: colors.0,
                to_color: colors.1,
                bloom,
                scale_factor: self.scale_factor,
                project: &project,
            },
            &mut self.particle_scratch,
        );
        if full {
            self.scene.set_walk_overlay(false, Vec::new(), &self.queue);
        }
        self.scene.set_walk_overlay(true, entries, &self.queue);
        if let Err(error) = self
            .particles
            .upload(&self.device, &self.queue, &self.particle_scratch)
        {
            tracing::warn!(%error, "route walk particles were not uploaded");
            self.particles.clear();
        }
        self.labels.mark_dirty();
        self.redraw_requested = true;
    }

    pub(super) fn walk_current_node(&self) -> Option<NodeId> {
        self.walk
            .as_ref()
            .map(|walk| walk.route.nodes()[walk.route.current()])
    }
}

fn traversal_curve(
    scene: &crate::gpu_scene::GpuScene,
    paths: &crate::path_layer::PreparedPathLayer,
    edge: EdgeId,
    from: NodeId,
    to: NodeId,
) -> Option<ParticleCurve> {
    let state = scene.state();
    let from_position = state.node_at_slot(state.node_slot(from)?)?.position;
    let to_position = state.node_at_slot(state.node_slot(to)?)?.position;
    let mut points = Vec::new();
    let has_path = paths.has_paths()
        && state
            .edge_slot(edge)
            .is_some_and(|slot| paths.edge_polyline(slot, &mut points));
    if !has_path {
        points = vec![from_position, to_position];
    } else if distance2(points[0], from_position)
        > distance2(*points.last().expect("polyline has points"), from_position)
    {
        points.reverse();
    }
    Some(ParticleCurve::new(points))
}

fn glow_bits(value: f32) -> u32 {
    ((value.clamp(0.0, 1.0) * 255.0).round() as u32) << WALK_GLOW_SHIFT
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn distance2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}
