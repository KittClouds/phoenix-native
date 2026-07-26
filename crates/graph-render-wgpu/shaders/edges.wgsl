struct CameraUniform {
    view_proj: mat4x4<f32>,
    eye_position: vec4<f32>,
    view_right: vec4<f32>,
    view_up: vec4<f32>,
    viewport_size: vec2<f32>,
    _padding: vec2<f32>,
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

struct GraphLensUniform {
    family_mask: vec2<u32>,
    scope_mask: vec2<u32>,
    relation_mask: vec2<u32>,
    review_mask: u32,
    product_index_enabled: u32,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(1) @binding(0) var<storage, read> nodes: array<NodeGpu>;
@group(2) @binding(0) var<storage, read> edges: array<EdgeGpu>;
@group(3) @binding(0) var<uniform> lens: GraphLensUniform;
@group(3) @binding(2) var<storage, read> edge_products: array<EdgeProductGpu>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) side: f32,
    @location(2) @interpolate(flat) visible: u32,
};

fn intersects(left: vec2<u32>, right: vec2<u32>) -> bool {
    return ((left.x & right.x) | (left.y & right.y)) != 0u;
}

fn edge_visible(product: EdgeProductGpu) -> bool {
    if (lens.product_index_enabled == 0u) {
        return true;
    }
    return product.enabled != 0u
        && intersects(product.family_mask, lens.family_mask)
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
    let is_visible = edge_visible(edge_products[instance_index]);
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
    let half_width = max(edge.width * 0.5, 0.5);
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
    color.a *= 1.0 - smoothstep(1.0 - derivative, 1.0, edge_distance);
    return color;
}
