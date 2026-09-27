struct CameraUniform {
    view_proj: mat4x4<f32>,
    eye_position: vec4<f32>,
    view_right: vec4<f32>,
    view_up: vec4<f32>,
    viewport_size: vec2<f32>,
    edge_opacity: f32,
    _padding: f32,
};

struct NodeGpu {
    position_radius: vec4<f32>,
    color: vec4<f32>,
    id_low: u32,
    id_high: u32,
    kind_flags: u32,
    _padding: u32,
};

struct EdgeGpu {
    source_slot: u32,
    target_slot: u32,
    kind_flags: u32,
    _padding0: u32,
    color: vec4<f32>,
    width: f32,
    id_low: u32,
    id_high: u32,
    _padding1: u32,
};

struct EdgeProductGpu {
    family_mask: vec2<u32>,
    scope_mask: vec2<u32>,
    relation_mask: vec2<u32>,
    review_mask: u32,
    enabled: u32,
};

struct NodeProductGpu {
    family_mask: vec2<u32>,
    scope_mask: vec2<u32>,
    review_mask: u32,
    enabled: u32,
    context_visible: u32,
    overlay_flags: u32,
};

struct GraphLensUniform {
    family_mask: vec2<u32>,
    entity_family_mask: vec2<u32>,
    topology_family_mask: vec2<u32>,
    scope_mask: vec2<u32>,
    relation_mask: vec2<u32>,
    review_mask: u32,
    product_index_enabled: u32,
    focus_active: u32,
    dimmed_node_opacity: f32,
    dimmed_edge_opacity: f32,
    topology_emphasis: u32,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(1) @binding(0) var<storage, read> nodes: array<NodeGpu>;
@group(2) @binding(0) var<storage, read> edges: array<EdgeGpu>;
@group(3) @binding(0) var<uniform> lens: GraphLensUniform;
@group(3) @binding(1) var<storage, read> node_products: array<NodeProductGpu>;
@group(3) @binding(2) var<storage, read> edge_products: array<EdgeProductGpu>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) side: f32,
    @location(2) @interpolate(flat) visible: u32,
    @location(3) @interpolate(flat) flags: u32,
    @location(4) progress: f32,
    @location(5) @interpolate(flat) emphasis_match: u32,
    @location(6) @interpolate(flat) navigation_flags: u32,
    @location(7) @interpolate(flat) source_state: u32,
    @location(8) @interpolate(flat) walk_state: u32,
};

// Source-local edge state derived from its endpoints: 0 = mode off,
// 1 = ghosted, 2 = both endpoints are in the verified source scope.
fn source_state(edge: EdgeGpu) -> u32 {
    let from_scope = node_products[edge.source_slot].overlay_flags;
    let to_scope = node_products[edge.target_slot].overlay_flags;
    if ((from_scope & 1u) == 0u) {
        return 0u;
    }
    return select(1u, 2u, (from_scope & 2u) != 0u && (to_scope & 2u) != 0u);
}

const SOURCE_GHOST_EDGE_OPACITY: f32 = 0.035;

// Route-walk edge state derived from its endpoints: 0 = no walk,
// 1 = dimmed context, 2 = route ahead, 3 = walked, 4 = traversal in flight.
// A frozen shortest route has no chords, so endpoint membership identifies
// route edges exactly.
fn walk_edge_state(edge: EdgeGpu) -> u32 {
    let a = node_products[edge.source_slot].overlay_flags;
    let b = node_products[edge.target_slot].overlay_flags;
    if ((a & 8u) == 0u) {
        return 0u;
    }
    if ((a & 240u) == 0u || (b & 240u) == 0u) {
        return 1u;
    }
    let both = a | b;
    if ((both & 64u) != 0u && (both & 128u) != 0u) {
        return 4u;
    }
    if ((a & 96u) != 0u && (b & 96u) != 0u) {
        return 3u;
    }
    return 2u;
}

const WALK_EDGE_MAX_WIDTH_PX: f32 = 2.4;

fn walk_edge_color(state: u32, color: vec4<f32>) -> vec4<f32> {
    switch state {
        case 1u: { return vec4<f32>(color.rgb, color.a * 0.018); }
        case 2u: { return vec4<f32>(mix(color.rgb, vec3<f32>(1.0), 0.06), 0.34); }
        case 3u: { return vec4<f32>(mix(color.rgb, vec3<f32>(1.0), 0.20), 0.66); }
        default: { return vec4<f32>(mix(color.rgb, vec3<f32>(1.0), 0.32), 0.82); }
    }
}


// Line width is relative to the smallest ordinary node, never to either endpoint.
// This keeps edges stable when degree makes centroids or medoids much larger.
const BASE_NODE_DIAMETER_PX: f32 = 2.0493;
const EDGE_WIDTH_SCALE: f32 = 0.90;
const MIN_EDGE_WIDTH_PX: f32 = 0.64;
const MAX_EDGE_TO_BASE_NODE_RATIO: f32 = 0.45;

fn intersects(left: vec2<u32>, right: vec2<u32>) -> bool {
    return ((left.x & right.x) | (left.y & right.y)) != 0u;
}

// Product pages carry detail lanes while the lens exposes broad families.
// Picking and rendering must use the same admission rule or valid node kinds
// disappear from hover even when their geometry is on screen.
fn family_visible(product_mask: vec2<u32>) -> bool {
    return intersects(product_mask, lens.family_mask);
}

fn emphasis_matches(primary_family: vec2<u32>) -> bool {
    switch lens.topology_emphasis {
        case 1u: { return (primary_family.x & 0x100u) != 0u; }
        case 2u: { return (primary_family.x & 0x200u) != 0u; }
        case 3u: { return (primary_family.x & 0x400u) != 0u; }
        default: { return false; }
    }
}

fn topology_lane_visible(product_mask: vec2<u32>) -> bool {
    let product_lanes = vec2<u32>(
        product_mask.x & 0xff000000u,
        product_mask.y & 0x0000003fu,
    );
    return (product_lanes.x | product_lanes.y) == 0u
        || intersects(product_lanes, lens.topology_family_mask);
}

fn primary_edge_family(product: EdgeProductGpu) -> vec2<u32> {
    let relation = product.relation_mask.x;
    if ((relation & 0x20u) != 0u) { // structural
        return vec2<u32>(0x100u | (product.family_mask.x & 0x7f000000u), 0u);
    }
    if ((relation & 0x01u) != 0u) { // co-occurrence / contextual discourse
        if ((product.family_mask.x & 0x400u) != 0u) {
            return vec2<u32>(0x400u, 0x20u);
        }
        return vec2<u32>(product.family_mask.x & 0xff0007ffu, product.family_mask.y & 0x3fu);
    }
    if ((relation & 0x02u) != 0u) { // observation
        return vec2<u32>(0x100u | (product.family_mask.x & 0x7f000000u), 0u);
    }
    if ((relation & 0x40u) != 0u) { return vec2<u32>(0x400u, 0x10u); } // identity
    if ((relation & 0x80u) != 0u) { return vec2<u32>(0x200u, 0x01u); } // relationship
    if ((relation & 0x100u) != 0u) { return vec2<u32>(0x80000200u, 0u); } // event
    if ((relation & 0x200u) != 0u) { return vec2<u32>(0x200u, 0x08u); } // memory
    if ((relation & 0x08u) != 0u) { return vec2<u32>(0x200u, 0x04u); } // causal
    if ((relation & 0x10u) != 0u) { return vec2<u32>(0x200u, 0x02u); } // temporal
    return vec2<u32>(product.family_mask.x & 0xff0007ffu, product.family_mask.y & 0x3fu);
}

fn edge_visible(edge: EdgeGpu, product: EdgeProductGpu) -> bool {
    if (lens.product_index_enabled == 0u) {
        return true;
    }
    let primary_family = primary_edge_family(product);
    return product.enabled != 0u
        && family_visible(primary_family)
        && topology_lane_visible(primary_family)
        && intersects(product.scope_mask, lens.scope_mask)
        && intersects(product.relation_mask, lens.relation_mask)
        && (product.review_mask & lens.review_mask) != 0u;
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let edge = edges[instance_index];
    let is_visible = edge_visible(edge, edge_products[instance_index]);
    let source_clip = camera.view_proj
        * vec4<f32>(nodes[edge.source_slot].position_radius.xyz, 1.0);
    let target_clip = camera.view_proj
        * vec4<f32>(nodes[edge.target_slot].position_radius.xyz, 1.0);
    let source_ndc = source_clip.xy / source_clip.w;
    let target_ndc = target_clip.xy / target_clip.w;
    let source_screen = (source_ndc * 0.5 + 0.5) * camera.viewport_size;
    let target_screen = (target_ndc * 0.5 + 0.5) * camera.viewport_size;
    let direction = target_screen - source_screen;
    let length_pixels = length(direction);
    let normal = select(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(-direction.y, direction.x) / length_pixels,
        length_pixels > 0.001,
    );
    let at_target = vertex_index >= 2u;
    let side = select(1.0, -1.0, vertex_index == 1u || vertex_index == 3u);
    let center = select(source_screen, target_screen, at_target);
    let navigation_scale = select(
        select(1.0, 1.16, (edge._padding0 & 1u) != 0u),
        1.32,
        (edge._padding0 & 2u) != 0u,
    );
    let walk_state = walk_edge_state(edge);
    let walk_route = walk_state >= 2u;
    let edge_width = select(
        clamp(
            edge.width * navigation_scale * EDGE_WIDTH_SCALE,
            MIN_EDGE_WIDTH_PX,
            BASE_NODE_DIAMETER_PX * MAX_EDGE_TO_BASE_NODE_RATIO,
        ),
        select(1.3, WALK_EDGE_MAX_WIDTH_PX, walk_state >= 3u),
        walk_route,
    );
    let half_width = edge_width * 0.5;
    let screen = center + normal * half_width * side;
    let ndc = (screen / camera.viewport_size - 0.5) * 2.0;
    let clip = select(source_clip, target_clip, at_target);

    var output: VertexOutput;
    output.position = select(
        vec4<f32>(2.0, 2.0, 2.0, 1.0),
        vec4<f32>(ndc * clip.w, clip.z, clip.w),
        is_visible,
    );
    let progress = select(0.0, 1.0, at_target);
    output.color = vec4<f32>(
        mix(nodes[edge.source_slot].color.rgb, nodes[edge.target_slot].color.rgb, progress),
        edge.color.a,
    );
    output.side = side;
    output.emphasis_match = select(0u, 1u, emphasis_matches(primary_edge_family(edge_products[instance_index])));
    output.navigation_flags = edge._padding0;
    output.source_state = source_state(edge);
    output.walk_state = walk_state;
    output.visible = select(0u, 1u, is_visible);
    output.flags = edge.kind_flags & 0xffffu;
    output.progress = progress;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.visible == 0u) {
        discard;
    }
    let edge_distance = abs(input.side);
    let derivative = max(fwidth(edge_distance), 0.0001);
    var color = input.color;
    color.a = min(color.a, camera.edge_opacity);
    if ((input.navigation_flags & 1u) != 0u) {
        color = vec4<f32>(mix(color.rgb, vec3<f32>(0.38, 0.78, 0.64), 0.14), color.a);
    }
    if ((input.navigation_flags & 2u) != 0u) {
        color = vec4<f32>(mix(color.rgb, vec3<f32>(0.90, 0.78, 0.51), 0.24), color.a);
    }
    if ((input.flags & 32768u) != 0u) {
        color.a = max(color.a, 0.86);
    } else if ((input.flags & 16384u) != 0u) {
        color.a = max(color.a, 0.42);
    }
    if (input.walk_state != 0u) {
        color = walk_edge_color(input.walk_state, color);
    } else if (input.source_state == 1u) {
        color.a *= SOURCE_GHOST_EDGE_OPACITY;
    } else if (input.source_state == 2u) {
        // In-scope edges keep normal rendering while the anchor is focused.
    } else if (lens.focus_active != 0u
        && (input.flags & 32768u) == 0u
        && (input.flags & 16384u) == 0u) {
        color.a *= lens.dimmed_edge_opacity;
    } else if (lens.focus_active == 0u && lens.topology_emphasis != 0u) {
        color.a *= select(0.60, 1.25, input.emphasis_match != 0u);
    }
    color.a *= mix(0.68, 1.0, input.progress);
    color.a *= 1.0 - smoothstep(1.0 - derivative, 1.0, edge_distance);
    return color;
}
