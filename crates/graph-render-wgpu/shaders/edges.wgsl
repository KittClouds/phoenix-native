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
    _padding: u32,
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
    _padding: u32,
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
};

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
    let edge_width = clamp(
        edge.width * EDGE_WIDTH_SCALE,
        MIN_EDGE_WIDTH_PX,
        BASE_NODE_DIAMETER_PX * MAX_EDGE_TO_BASE_NODE_RATIO,
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
    output.color = edge.color;
    output.side = side;
    output.visible = select(0u, 1u, is_visible);
    output.flags = edge.kind_flags & 0xffffu;
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
    if ((input.flags & 32768u) != 0u) {
        color = mix(color, vec4<f32>(1.0, 0.147, 0.022, 0.95), 0.82);
    } else if ((input.flags & 16384u) != 0u) {
        color = mix(color, vec4<f32>(0.040, 0.672, 0.420, 0.72), 0.54);
    }
    if (lens.focus_active != 0u
        && (input.flags & 32768u) == 0u
        && (input.flags & 16384u) == 0u) {
        color.a *= lens.dimmed_edge_opacity;
    }
    color.a *= 1.0 - smoothstep(1.0 - derivative, 1.0, edge_distance);
    return color;
}
