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

struct NodeProductGpu {
    family_mask: vec2<u32>,
    scope_mask: vec2<u32>,
    review_mask: u32,
    enabled: u32,
    _padding: vec2<u32>,
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
@group(2) @binding(0) var<uniform> lens: GraphLensUniform;
@group(2) @binding(1) var<storage, read> node_products: array<NodeProductGpu>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) encoded_slot: u32,
    @location(2) @interpolate(flat) visible: u32,
};

// Must match nodes.wgsl so the visible sphere and its pick target scale together.
const NODE_SCREEN_SCALE: f32 = 1.3662;
const NODE_DIAMETER_SCALE: f32 = 2.02;
const NODE_MIN_DIAMETER_PX: f32 = 3.50;
const NODE_MAX_DIAMETER_PX: f32 = 34.0;
const VISUAL_ROLE_MASK: u32 = 3840u;
const VISUAL_ROLE_SHIFT: u32 = 8u;

fn visual_role(flags: u32) -> u32 {
    return (flags & VISUAL_ROLE_MASK) >> VISUAL_ROLE_SHIFT;
}

fn role_scale(role: u32) -> f32 {
    switch role {
        case 1u: { return 1.72; }
        case 2u: { return 1.22; }
        case 3u: { return 1.48; }
        case 4u: { return 1.95; }
        case 5u: { return 1.82; }
        default: { return 1.0; }
    }
}

fn intersects(left: vec2<u32>, right: vec2<u32>) -> bool {
    return ((left.x & right.x) | (left.y & right.y)) != 0u;
}

// Keep picking admission identical to the visible node pass.  Product pages
// carry detail lanes (character/location/event/etc.) while the active lens
// usually selects the broad family bit.  A plain mask intersection makes
// those nodes unpickable even though the visible pass correctly renders them.
fn family_visible(product_mask: vec2<u32>) -> bool {
    if (intersects(product_mask, lens.family_mask)) {
        return true;
    }
    let entity_detail = (product_mask.x & 0x00ff0000u) != 0u
        && (lens.family_mask.x & 0x000000ffu) != 0u;
    let structure_detail = (product_mask.x & 0x7f000000u) != 0u
        && (lens.family_mask.x & 0x00000100u) != 0u;
    let fact_detail = ((product_mask.x & 0x80000000u) != 0u
        || (product_mask.y & 0x0000001fu) != 0u)
        && (lens.family_mask.x & 0x00000200u) != 0u;
    let discourse_detail = (product_mask.y & 0x00000030u) != 0u
        && (lens.family_mask.x & 0x00000400u) != 0u;
    return entity_detail || structure_detail || fact_detail || discourse_detail;
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
    if (lens.product_index_enabled == 0u) {
        return true;
    }
    return product.enabled != 0u
        && family_visible(product.family_mask)
        && entity_lane_visible(product.family_mask)
        && topology_lane_visible(product.family_mask)
        && intersects(product.scope_mask, lens.scope_mask)
        && (product.review_mask & lens.review_mask) != 0u;
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let node = nodes[instance_index];
    let is_visible = node_visible(node_products[instance_index]);
    let corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0,  1.0),
    );
    let uv = corners[vertex_index];
    let flags = node.kind_flags & 0xffffu;
    let visual_diameter = clamp(
        node.position_radius.w * NODE_DIAMETER_SCALE * role_scale(visual_role(flags)),
        NODE_MIN_DIAMETER_PX,
        NODE_MAX_DIAMETER_PX,
    ) * NODE_SCREEN_SCALE;
    let hit_diameter = clamp(visual_diameter * 0.7 + 6.0, 7.0, 18.0);
    let view_back = cross(camera.view_right.xyz, camera.view_up.xyz);
    let view_depth = max(
        dot(camera.eye_position.xyz - node.position_radius.xyz, view_back),
        0.01,
    );
    let world_per_pixel = view_depth * 0.8284271 / max(camera.viewport_size.y, 1.0);
    let world_radius = hit_diameter * 0.5 * world_per_pixel;
    let world = node.position_radius.xyz
        + (camera.view_right.xyz * uv.x + camera.view_up.xyz * uv.y) * world_radius;

    var output: VertexOutput;
    output.position = select(
        vec4<f32>(2.0, 2.0, 2.0, 1.0),
        camera.view_proj * vec4<f32>(world, 1.0),
        is_visible,
    );
    output.uv = uv;
    output.encoded_slot = instance_index + 1u;
    output.visible = select(0u, 1u, is_visible);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) u32 {
    if (input.visible == 0u || length(input.uv) > 1.0) {
        discard;
    }
    return input.encoded_slot;
}
