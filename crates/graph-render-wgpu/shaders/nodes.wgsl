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
    _padding: vec4<u32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(1) @binding(0) var<storage, read> nodes: array<NodeGpu>;
@group(2) @binding(0) var<uniform> lens: GraphLensUniform;
@group(2) @binding(1) var<storage, read> node_products: array<NodeProductGpu>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) flags: u32,
    @location(3) @interpolate(flat) visible: u32,
    @location(4) @interpolate(flat) kind: u32,
};

// Screen-space geometry contract. Semantic radius may grow for hubs, centroids,
// and medoids, but line width is governed independently in the edge shaders.
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
        case 1u: { return 1.72; } // root/document or episode
        case 2u: { return 1.22; } // anchor/entity or evidence
        case 3u: { return 1.48; } // deterministic high-degree hub
        case 4u: { return 1.95; } // producer-provided medoid
        case 5u: { return 1.82; } // producer-provided centroid
        default: { return 1.0; }
    }
}

fn role_aura_strength(role: u32) -> f32 {
    switch role {
        case 1u: { return 0.24; }
        case 2u: { return 0.20; }
        case 3u: { return 0.23; }
        case 4u: { return 0.28; }
        case 5u: { return 0.26; }
        default: { return 0.14; }
    }
}

fn intersects(left: vec2<u32>, right: vec2<u32>) -> bool {
    return ((left.x & right.x) | (left.y & right.y)) != 0u;
}

fn entity_lane_visible(product_mask: vec2<u32>) -> bool {
    let product_lanes = product_mask.x & 0x00ff0000u;
    return product_lanes == 0u
        || (product_lanes & lens.entity_family_mask.x) != 0u;
}

fn family_visible(product_mask: vec2<u32>) -> bool {
    let selected = lens.family_mask;
    if ((product_mask.x & selected.x) | (product_mask.y & selected.y)) != 0u {
        return true;
    }
    let entity_detail = (product_mask.x & 0x00ff0000u) != 0u
        && (selected.x & 0x000000ffu) != 0u;
    let structure_detail = (product_mask.x & 0x7f000000u) != 0u
        && (selected.x & 0x00000100u) != 0u;
    let fact_detail = ((product_mask.x & 0x80000000u) != 0u
        || (product_mask.y & 0x0000001fu) != 0u)
        && (selected.x & 0x00000200u) != 0u;
    let discourse_detail = (product_mask.y & 0x00000030u) != 0u
        && (selected.x & 0x00000400u) != 0u;
    return entity_detail || structure_detail || fact_detail || discourse_detail;
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

// Product pages carry the authoritative high-bit entity lanes. Keep this
// separate from the legacy node kind so structure/fact nodes can inherit the
// same family aura as the entity they explain without changing their semantic
// kind or their body color.
fn entity_lane_kind(mask: vec2<u32>) -> u32 {
    let lanes = mask.x;
    if ((lanes & 0x00010000u) != 0u) {
        return 101u; // character/person
    }
    if ((lanes & 0x00020000u) != 0u) {
        return 102u; // location
    }
    if ((lanes & 0x00040000u) != 0u) {
        return 103u; // network
    }
    if ((lanes & 0x00080000u) != 0u) {
        return 104u; // creature
    }
    if ((lanes & 0x00100000u) != 0u) {
        return 105u; // npc
    }
    return 0u;
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
    let uv = corners[vertex_index] * 1.24;
    let flags = node.kind_flags & 0xffffu;
    let role = visual_role(flags);
    let diameter_pixels = clamp(
        node.position_radius.w * NODE_DIAMETER_SCALE * role_scale(role),
        NODE_MIN_DIAMETER_PX,
        NODE_MAX_DIAMETER_PX,
    ) * NODE_SCREEN_SCALE;
    let view_back = cross(camera.view_right.xyz, camera.view_up.xyz);
    let view_depth = max(
        dot(camera.eye_position.xyz - node.position_radius.xyz, view_back),
        0.01,
    );
    let world_per_pixel = view_depth * 0.8284271 / max(camera.viewport_size.y, 1.0);
    let world_radius = diameter_pixels * 0.5 * world_per_pixel;
    let world = node.position_radius.xyz
        + (camera.view_right.xyz * uv.x + camera.view_up.xyz * uv.y) * world_radius;

    var output: VertexOutput;
    output.position = select(
        vec4<f32>(2.0, 2.0, 2.0, 1.0),
        camera.view_proj * vec4<f32>(world, 1.0),
        is_visible,
    );
    output.uv = uv;
    output.color = node.color;
    output.flags = flags;
    output.visible = select(0u, 1u, is_visible);
    var kind = node.kind_flags >> 16u;
    // An unfiltered renderer uses the sentinel all-ones product page.  Do
    // not interpret that sentinel as a character lane; only an installed,
    // authoritative product index may override the node's own kind.
    if (lens.product_index_enabled != 0u) {
        let lane_kind = entity_lane_kind(node_products[instance_index].family_mask);
        if (lane_kind != 0u) {
            kind = lane_kind;
        }
    }
    output.kind = kind;
    return output;
}

fn type_aura(kind: u32, fallback: vec3<f32>) -> vec3<f32> {
    // Entity kinds use a stable family hue for the halo; the node body keeps
    // its palette-selected color. Structural/semantic nodes fall back to
    // their own color so the aura never introduces a white bloom.
    switch kind {
        case 1u, 101u: { return vec3<f32>(0.10, 0.30, 0.92); } // character/person
        case 2u, 102u: { return vec3<f32>(0.02, 0.72, 0.48); } // location
        case 3u, 105u: { return vec3<f32>(0.58, 0.18, 0.92); } // npc
        case 4u, 7u, 103u: { return vec3<f32>(0.02, 0.65, 0.82); } // network/faction
        case 8u, 104u: { return vec3<f32>(0.92, 0.42, 0.08); } // creature
        default: { return fallback; }
    }
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.visible == 0u) {
        discard;
    }
    let distance = length(input.uv);
    let derivative = max(fwidth(distance), 0.0001);
    let circle = 1.0 - smoothstep(1.0 - derivative, 1.0 + derivative, distance);
    let aura = 1.0 - smoothstep(0.92, 1.52, distance);
    let role = visual_role(input.flags);
    let hovered = (input.flags & 4096u) != 0u;
    let selected = (input.flags & 8192u) != 0u;
    let neighbor = (input.flags & 16384u) != 0u;
    let route = (input.flags & 32768u) != 0u;
    if (circle <= 0.001 && aura <= 0.001 && !hovered && !selected) {
        discard;
    }

    var color = input.color;
    let sphere_xy = input.uv * min(1.0, 1.0 / max(distance, 0.0001));
    let sphere_z = sqrt(max(0.0, 1.0 - dot(sphere_xy, sphere_xy)));
    let sphere_normal = normalize(vec3<f32>(sphere_xy, sphere_z));
    let light_direction = normalize(vec3<f32>(-0.48, 0.62, 0.62));
    let half_direction = normalize(light_direction + vec3<f32>(0.0, 0.0, 1.0));
    let diffuse = 0.52 + 0.48 * max(dot(sphere_normal, light_direction), 0.0);
    let specular = pow(max(dot(sphere_normal, half_direction), 0.0), 28.0) * 0.16;
    let rim = pow(1.0 - sphere_z, 2.0) * 0.12;
    let sphere_rgb = min(
        color.rgb * (diffuse + rim) + vec3<f32>(specular),
        vec3<f32>(1.0),
    );
    let aura_rgb = mix(color.rgb, type_aura(input.kind, color.rgb), 0.34);
    let halo_alpha = color.a * aura * role_aura_strength(role);
    color = vec4<f32>(mix(aura_rgb, sphere_rgb, circle), max(color.a * circle, halo_alpha));
    if (route) {
        let glow = 1.0 - smoothstep(0.55, 1.15, distance);
        color = mix(color, vec4<f32>(0.913, 0.195, 0.033, 1.0), glow * 0.72);
    } else if (selected) {
        let ring = smoothstep(0.96 - derivative, 1.0, distance)
            - smoothstep(1.20 - derivative, 1.24, distance);
        color = mix(color, vec4<f32>(1.0, 0.578, 0.022, 1.0), ring);
    } else if (hovered) {
        let ring = smoothstep(0.97 - derivative, 1.0, distance)
            - smoothstep(1.10 - derivative, 1.14, distance);
        color = mix(color, vec4<f32>(0.019, 0.890, 0.745, 1.0), ring);
    } else if (neighbor) {
        color = mix(color, vec4<f32>(0.064, 0.749, 0.477, color.a), 0.32);
    }
    return color;
}
