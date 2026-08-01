struct CameraUniform {
    view_proj: mat4x4<f32>,
    eye_position: vec4<f32>,
    view_right: vec4<f32>,
    view_up: vec4<f32>,
    viewport_size: vec2<f32>,
    edge_opacity: f32,
    _padding: f32,
};

struct PreparedSegmentGpu {
    start: vec4<f32>,
    end: vec4<f32>,
    color: vec4<f32>,
    edge_slot: u32,
    flags: u32,
    _padding: vec2<u32>,
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
    _padding: vec2<u32>,
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

struct GraphLensUniform {
    family_mask: vec2<u32>,
    entity_family_mask: vec2<u32>,
    topology_family_mask: vec2<u32>,
    scope_mask: vec2<u32>,
    relation_mask: vec2<u32>,
    review_mask: u32,
    product_index_enabled: u32,
    _padding: vec4<u32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(1) @binding(0) var<storage, read> segments: array<PreparedSegmentGpu>;
@group(2) @binding(0) var<uniform> lens: GraphLensUniform;
@group(2) @binding(1) var<storage, read> node_products: array<NodeProductGpu>;
@group(2) @binding(2) var<storage, read> edge_products: array<EdgeProductGpu>;
@group(3) @binding(0) var<storage, read> edges: array<EdgeGpu>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) side: f32,
    @location(2) @interpolate(flat) visible: u32,
    @location(3) @interpolate(flat) edge_slot: u32,
    @location(4) @interpolate(flat) segment_flags: u32,
};

// Prepared paths and direct edges share the same bounded screen-space width.
// The reference is an ordinary node, so semantic node growth cannot widen lines.
const BASE_NODE_DIAMETER_PX: f32 = 2.0493;
const EDGE_WIDTH_SCALE: f32 = 0.90;
const MIN_EDGE_WIDTH_PX: f32 = 0.64;
const MAX_EDGE_TO_BASE_NODE_RATIO: f32 = 0.45;

fn intersects(left: vec2<u32>, right: vec2<u32>) -> bool {
    return ((left.x & right.x) | (left.y & right.y)) != 0u;
}

fn entity_lane_visible(product_mask: vec2<u32>) -> bool {
    let product_lanes = product_mask.x & 0x00ff0000u;
    return product_lanes == 0u
        || (product_lanes & lens.entity_family_mask.x) != 0u;
}

fn topology_lane_visible(product_mask: vec2<u32>) -> bool {
    let product_lanes = vec2<u32>(
        product_mask.x & 0xff000000u,
        product_mask.y & 0x0000003fu,
    );
    return (product_lanes.x | product_lanes.y) == 0u
        || intersects(product_lanes, lens.topology_family_mask);
}

fn node_visible(product: NodeProductGpu) -> bool {
    return product.enabled != 0u
        && intersects(product.family_mask, lens.family_mask)
        && entity_lane_visible(product.family_mask)
        && topology_lane_visible(product.family_mask)
        && intersects(product.scope_mask, lens.scope_mask)
        && (product.review_mask & lens.review_mask) != 0u;
}

fn visible(segment: PreparedSegmentGpu) -> bool {
    if ((segment.flags & 1u) != 0u || lens.product_index_enabled == 0u) {
        return true;
    }
    let product = edge_products[segment.edge_slot];
    let edge = edges[segment.edge_slot];
    return product.enabled != 0u
        && intersects(product.family_mask, lens.family_mask)
        && entity_lane_visible(product.family_mask)
        && topology_lane_visible(product.family_mask)
        && intersects(product.scope_mask, lens.scope_mask)
        && intersects(product.relation_mask, lens.relation_mask)
        && (product.review_mask & lens.review_mask) != 0u
        && node_visible(node_products[edge.source_slot])
        && node_visible(node_products[edge.target_slot]);
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let segment = segments[instance_index];
    let source_clip = camera.view_proj * vec4<f32>(segment.start.xyz, 1.0);
    let target_clip = camera.view_proj * vec4<f32>(segment.end.xyz, 1.0);
    let source_screen = (source_clip.xy / source_clip.w * 0.5 + 0.5) * camera.viewport_size;
    let target_screen = (target_clip.xy / target_clip.w * 0.5 + 0.5) * camera.viewport_size;
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
    let clip = select(source_clip, target_clip, at_target);
    let segment_width = clamp(
        segment.start.w * EDGE_WIDTH_SCALE,
        MIN_EDGE_WIDTH_PX,
        BASE_NODE_DIAMETER_PX * MAX_EDGE_TO_BASE_NODE_RATIO,
    );
    let screen = center + normal * segment_width * 0.5 * side;
    let ndc = (screen / camera.viewport_size - 0.5) * 2.0;
    let is_visible = visible(segment);

    var output: VertexOutput;
    output.position = select(
        vec4<f32>(2.0, 2.0, 2.0, 1.0),
        vec4<f32>(ndc * clip.w, clip.z, clip.w),
        is_visible,
    );
    output.color = segment.color;
    output.side = side;
    output.visible = select(0u, 1u, is_visible);
    output.edge_slot = segment.edge_slot;
    output.segment_flags = segment.flags;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.visible == 0u) {
        discard;
    }
    let derivative = max(fwidth(abs(input.side)), 0.0001);
    var color = input.color;
    color.a = min(color.a, camera.edge_opacity);
    if ((input.segment_flags & 1u) == 0u) {
        let runtime_flags = edges[input.edge_slot].kind_flags & 0xffffu;
        if ((runtime_flags & 32768u) != 0u) {
            color = mix(color, vec4<f32>(1.0, 0.147, 0.022, 0.96), 0.84);
        } else if ((runtime_flags & 16384u) != 0u) {
            color = mix(color, vec4<f32>(0.040, 0.672, 0.420, 0.76), 0.56);
        }
    }
    color.a *= 1.0 - smoothstep(1.0 - derivative, 1.0, abs(input.side));
    return color;
}
