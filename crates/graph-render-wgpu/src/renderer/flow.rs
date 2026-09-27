//! Document flow presentation for [`GraphRenderer`].
//!
//! Waves leave every document together; each wave carries particles along
//! its branches and lights the nodes it reaches. The flow plays once, then
//! settles with the reached structure lit and draws no further frames.

use super::GraphRenderer;
use crate::flow_walk::{build_flow, FlowBranch, RANK_NONE};
use crate::particles::{ParticleCurve, ParticleGpu};
use crate::{FlowStatus, WALK_CURRENT, WALK_GLOW_SHIFT, WALK_NEXT, WALK_VISITED};
use std::collections::HashMap;

/// Seconds the documents glow before the first wave leaves.
const LAUNCH_SECS: f32 = 0.72;
/// Seconds one wave takes to cross its edges.
const TRAVEL_SECS: f32 = 1.28;
/// Seconds a wave rests on the nodes it reached.
const HOLD_SECS: f32 = 0.22;
const HOP_SECS: f32 = TRAVEL_SECS + HOLD_SECS;
/// Seconds an arrival blooms before settling into afterglow.
const BLOOM_SECS: f32 = 0.9;
/// Seconds after the last wave before the flow settles.
const SETTLE_SECS: f32 = 1.15;

pub(crate) struct ActiveFlow {
    branches: Vec<FlowBranch>,
    curves: Vec<ParticleCurve>,
    colors: Vec<([f32; 4], [f32; 4])>,
    roots: Vec<u32>,
    /// Node slot and the wave that first reaches it.
    arrivals: Vec<(u32, u32)>,
    levels: u32,
    elapsed: f32,
    settled: bool,
}

impl ActiveFlow {
    fn duration(&self) -> f32 {
        LAUNCH_SECS + self.levels as f32 * HOP_SECS + SETTLE_SECS
    }

    pub(super) fn status(&self) -> FlowStatus {
        FlowStatus {
            documents: self.roots.len(),
            connections: self.branches.len(),
            levels: self.levels,
            settled: self.settled,
        }
    }
}

impl GraphRenderer {
    /// Starts the document flow over the currently visible graph. It replaces
    /// any route walk; both are display-only overlays.
    pub fn start_document_flow(&mut self) -> &crate::RouteWalkStatus {
        self.end_route_walk(None);
        self.flow = None;
        let (ranks, ids, edges) = self.scene.flow_inputs();
        let branches = build_flow(&ranks, &ids, &edges);
        let roots: Vec<u32> = ranks
            .iter()
            .enumerate()
            .filter(|&(_, &rank)| rank == 0)
            .map(|(slot, _)| slot as u32)
            .collect();
        if branches.is_empty() {
            self.clear_flow_presentation();
            return self.publish_walk_status(Some(crate::RouteWalkNotice::NoDocumentFlow));
        }
        let mut arrivals: HashMap<u32, u32> = HashMap::new();
        for branch in &branches {
            let wave = arrivals.entry(branch.target).or_insert(branch.depth);
            *wave = (*wave).min(branch.depth);
        }
        let mut arrivals: Vec<(u32, u32)> = arrivals
            .into_iter()
            .filter(|(slot, _)| ranks[*slot as usize] != 0)
            .collect();
        arrivals.sort_unstable();
        let levels = branches.iter().map(|branch| branch.depth + 1).max().unwrap_or(0);
        let wanted: Vec<u32> = branches.iter().map(|branch| branch.edge).collect();
        let polylines = if self.prepared_paths.has_paths() {
            self.prepared_paths.edge_polylines(&wanted)
        } else {
            HashMap::new()
        };
        let state = self.scene.state();
        let position = |slot: u32| state.node_at_slot(slot).map(|node| node.position);
        let color = |slot: u32| self.scene.node_color(slot).unwrap_or([1.0; 4]);
        let mut curves = Vec::with_capacity(branches.len());
        let mut colors = Vec::with_capacity(branches.len());
        for branch in &branches {
            let from = position(branch.source).unwrap_or([0.0; 3]);
            let to = position(branch.target).unwrap_or(from);
            let mut points = polylines
                .get(&branch.edge)
                .cloned()
                .unwrap_or_else(|| vec![from, to]);
            if distance2(points[0], from) > distance2(points[points.len() - 1], from) {
                points.reverse();
            }
            curves.push(ParticleCurve::new(points));
            colors.push((color(branch.source), color(branch.target)));
        }
        self.flow = Some(ActiveFlow {
            branches,
            curves,
            colors,
            roots: roots.into_iter().filter(|slot| ranks[*slot as usize] != RANK_NONE).collect(),
            arrivals,
            levels,
            elapsed: 0.0,
            settled: false,
        });
        self.scene.set_walk_overlay(false, Vec::new(), &self.queue);
        self.refresh_flow_presentation();
        self.publish_walk_status(None)
    }

    pub fn exit_document_flow(&mut self) -> &crate::RouteWalkStatus {
        if self.flow.take().is_some() {
            self.clear_flow_presentation();
        }
        self.publish_walk_status(None)
    }

    pub(super) fn flow_animating(&self) -> bool {
        self.flow.as_ref().is_some_and(|flow| !flow.settled)
    }

    pub(super) fn advance_document_flow(&mut self, elapsed: f32) {
        let Some(flow) = self.flow.as_mut() else {
            return;
        };
        if flow.settled {
            return;
        }
        flow.elapsed += elapsed.clamp(0.0, 0.1);
        let settled = flow.elapsed >= flow.duration();
        flow.settled = settled;
        self.refresh_flow_presentation();
        if settled {
            self.publish_walk_status(None);
        }
    }

    /// Scene or visibility changed under the flow; its branches no longer
    /// describe the visible graph, so it ends.
    pub(super) fn end_document_flow(&mut self) {
        if self.flow.take().is_some() {
            self.clear_flow_presentation();
            self.publish_walk_status(None);
        }
    }

    fn clear_flow_presentation(&mut self) {
        self.scene.set_walk_overlay(false, Vec::new(), &self.queue);
        self.particles.clear();
        self.labels.mark_dirty();
        self.redraw_requested = true;
    }

    fn refresh_flow_presentation(&mut self) {
        let Some(flow) = self.flow.as_ref() else {
            return;
        };
        let t = flow.elapsed;
        let mut entries = Vec::with_capacity(flow.roots.len() + flow.arrivals.len());
        let launch_glow = if flow.settled {
            0.35
        } else {
            (1.0 - (t - LAUNCH_SECS).max(0.0) / BLOOM_SECS).clamp(0.35, 1.0)
        };
        for &root in &flow.roots {
            entries.push((root, WALK_CURRENT | glow_bits(launch_glow)));
        }
        for &(slot, wave) in &flow.arrivals {
            let start = LAUNCH_SECS + wave as f32 * HOP_SECS;
            let arrive = start + TRAVEL_SECS;
            let bits = if flow.settled || t >= arrive + BLOOM_SECS {
                WALK_VISITED
            } else if t >= arrive {
                WALK_NEXT | glow_bits(1.0 - (t - arrive) / BLOOM_SECS)
            } else if t >= start {
                WALK_NEXT | glow_bits(smoothstep(0.72, 1.0, (t - start) / TRAVEL_SECS))
            } else {
                continue;
            };
            entries.push((slot, bits));
        }

        let mut particles = std::mem::take(&mut self.particle_scratch);
        particles.clear();
        if !flow.settled {
            let scale = self.scale_factor.max(0.5);
            for (index, branch) in flow.branches.iter().enumerate() {
                let local = t - LAUNCH_SECS - branch.depth as f32 * HOP_SECS;
                if !(0.0..=TRAVEL_SECS).contains(&local) {
                    continue;
                }
                let progress = local / TRAVEL_SECS;
                let along = smoothstep(0.06, 0.94, progress);
                let fade = (std::f32::consts::PI * progress).sin().powf(0.4);
                let (from, to) = flow.colors[index];
                for (offset, radius, white, alpha) in
                    [(0.0, 3.0, 0.5, 0.95), (0.07, 1.8, 0.25, 0.5)]
                {
                    let at = (along - offset).max(0.0);
                    let point = flow.curves[index].sample(at);
                    let tint = mix(from, to, at);
                    particles.push(ParticleGpu {
                        position_size: [point[0], point[1], point[2], radius * scale],
                        color: [
                            tint[0] + (1.0 - tint[0]) * white,
                            tint[1] + (1.0 - tint[1]) * white,
                            tint[2] + (1.0 - tint[2]) * white,
                            alpha * fade,
                        ],
                        offset: [0.0; 4],
                    });
                }
            }
        }
        self.scene.set_walk_overlay(true, entries, &self.queue);
        if particles.is_empty() {
            self.particles.clear();
        } else if let Err(error) = self.particles.upload(&self.device, &self.queue, &particles) {
            tracing::warn!(%error, "document flow particles were not uploaded");
            self.particles.clear();
        }
        self.particle_scratch = particles;
        self.labels.mark_dirty();
        self.redraw_requested = true;
    }
}

/// Whole waves land at once, so arrivals bloom at a fraction of the route
/// walk's single-node bloom to keep dense waves from washing out.
const FLOW_BLOOM_SCALE: f32 = 0.6;

fn glow_bits(value: f32) -> u32 {
    ((value.clamp(0.0, 1.0) * FLOW_BLOOM_SCALE * 255.0).round() as u32) << WALK_GLOW_SHIFT
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn distance2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}
