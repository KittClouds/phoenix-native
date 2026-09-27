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
@group(2) @binding(0) var<uniform> lens: GraphLensUniform;
@group(2) @binding(1) var<storage, read> node_products: array<NodeProductGpu>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) flags: u32,
    @location(3) @interpolate(flat) visible: u32,
    @location(4) @interpolate(flat) kind: u32,
    @location(5) @interpolate(flat) context_only: u32,
    @location(6) @interpolate(flat) emphasis_match: u32,
    @location(7) @interpolate(flat) overlay_flags: u32,
};

// Source-local display: out-of-scope nodes stay resident but recede.
const SOURCE_GHOST_NODE_OPACITY: f32 = 0.09;

// Route walk overlay bits (see lens.rs WALK_*).
const WALK_ACTIVE: u32 = 8u;
const WALK_ROUTE: u32 = 16u;
const WALK_VISITED: u32 = 32u;
const WALK_CURRENT: u32 = 64u;
const WALK_NEXT: u32 = 128u;
const WALK_MEMBER: u32 = 240u;
// Non-route nodes remain only as faint spatial context during a walk.
const WALK_CONTEXT_OPACITY: f32 = 0.075;

fn walk_lit(bits: u32) -> bool {
    return (bits & WALK_ACTIVE) != 0u && (bits & WALK_MEMBER) != 0u;
}

fn walk_glow(bits: u32) -> f32 {
    return f32((bits >> 16u) & 255u) / 255.0;
}

// Reader glow (4B): additive light on objects bound to the spoken segment.
const READER_GLOW: u32 = 256u;
// Story timeline (4C): untimed objects and the bloom of new arrivals.
const STORY_UNTIMED: u32 = 512u;

fn reader_glow(bits: u32) -> f32 {
    // The story's arrival bloom reuses the reader halo.
    let bloom = f32((bits >> 12u) & 15u) / 15.0;
    if ((bits & READER_GLOW) == 0u) {
        return bloom;
    }
    return max(f32((bits >> 24u) & 255u) / 255.0, bloom);
}

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
    // Semantic products own their structure/fact/discourse identity. Their
    // entity bits are secondary aura context and never gate their body.
    if ((product_mask.x & 0x00000700u) != 0u) {
        return true;
    }
    let product_lanes = product_mask.x & 0x00ff0000u;
    return product_lanes == 0u
        || (product_lanes & lens.entity_family_mask.x) != 0u;
}

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

fn primary_node_family(mask: vec2<u32>) -> vec2<u32> {
    if ((mask.x & 0x01000000u) != 0u) { return vec2<u32>(0x01000100u, 0u); }
    if ((mask.x & 0x02000000u) != 0u) { return vec2<u32>(0x02000100u, 0u); }
    if ((mask.x & 0x10000000u) != 0u) { return vec2<u32>(0x10000100u, 0u); }
    if ((mask.x & 0x20000000u) != 0u) { return vec2<u32>(0x20000100u, 0u); }
    if ((mask.x & 0x40000000u) != 0u) { return vec2<u32>(0x40000100u, 0u); }
    if ((mask.x & 0x04000000u) != 0u) { return vec2<u32>(0x04000100u, 0u); }
    if ((mask.x & 0x08000000u) != 0u) { return vec2<u32>(0x08000100u, 0u); }
    if ((mask.x & 0x80000000u) != 0u) { return vec2<u32>(0x80000200u, 0u); }
    if ((mask.y & 0x01u) != 0u) { return vec2<u32>(0x200u, 0x01u); }
    if ((mask.y & 0x02u) != 0u) { return vec2<u32>(0x200u, 0x02u); }
    if ((mask.y & 0x04u) != 0u) { return vec2<u32>(0x200u, 0x04u); }
    if ((mask.y & 0x08u) != 0u) { return vec2<u32>(0x200u, 0x08u); }
    if ((mask.y & 0x10u) != 0u) { return vec2<u32>(0x400u, 0x10u); }
    if ((mask.y & 0x20u) != 0u) { return vec2<u32>(0x400u, 0x20u); }
    if ((mask.x & 0x100u) != 0u) { return vec2<u32>(0x100u, 0u); }
    if ((mask.x & 0x200u) != 0u) { return vec2<u32>(0x200u, 0u); }
    if ((mask.x & 0x400u) != 0u) { return vec2<u32>(0x400u, 0u); }
    return vec2<u32>(mask.x & 0xffu, 0u);
}

fn node_primary_visible(product: NodeProductGpu) -> bool {
    if (lens.product_index_enabled == 0u) {
        return true;
    }
    let primary_family = primary_node_family(product.family_mask);
    return product.enabled != 0u
        && family_visible(primary_family)
        && entity_lane_visible(product.family_mask)
        && topology_lane_visible(primary_family)
        && intersects(product.scope_mask, lens.scope_mask)
        && (product.review_mask & lens.review_mask) != 0u;
}

fn node_context_visible(product: NodeProductGpu) -> bool {
    let primary_family = primary_node_family(product.family_mask);
    return product.context_visible != 0u
        && product.enabled != 0u
        && entity_lane_visible(product.family_mask)
        && topology_lane_visible(primary_family)
        && intersects(product.scope_mask, lens.scope_mask)
        && (product.review_mask & lens.review_mask) != 0u;
}

fn node_visible(product: NodeProductGpu) -> bool {
    return node_primary_visible(product) || node_context_visible(product);
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
    let product = node_products[instance_index];
    let is_primary = node_primary_visible(product);
    let is_visible = is_primary || node_context_visible(product);
    let corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0,  1.0),
    );
    let walk_bits = product.overlay_flags;
    let lit = walk_lit(walk_bits);
    // Route members get a wider quad so their bloom halo is never clipped.
    let spoken = reader_glow(walk_bits);
    let uv = corners[vertex_index] * select(1.24, 2.6, lit || spoken > 0.0);
    var walk_scale = 1.0;
    if (lit && (walk_bits & WALK_CURRENT) != 0u) {
        walk_scale = 1.3 + 0.45 * walk_glow(walk_bits);
    } else if (lit && (walk_bits & WALK_NEXT) != 0u) {
        walk_scale = 1.0 + 0.3 * walk_glow(walk_bits);
    }
    let flags = node.kind_flags & 0xffffu;
    let role = visual_role(flags);
    let emphasized = emphasis_matches(primary_node_family(product.family_mask));
    var diameter_pixels = clamp(
        node.position_radius.w * NODE_DIAMETER_SCALE * role_scale(role),
        NODE_MIN_DIAMETER_PX,
        NODE_MAX_DIAMETER_PX,
    ) * NODE_SCREEN_SCALE * walk_scale;
    if (!lit && spoken > 0.0 && (walk_bits & WALK_ACTIVE) == 0u) {
        // Spoken nodes get a floor on screen size so they read at full-graph
        // density, where ordinary dots are only a few pixels wide.
        diameter_pixels = max(
            diameter_pixels * (1.0 + 0.6 * spoken),
            (6.0 + 10.0 * spoken) * NODE_SCREEN_SCALE,
        );
    }
    let view_back = cross(camera.view_right.xyz, camera.view_up.xyz);
    let view_depth = max(
        dot(camera.eye_position.xyz - node.position_radius.xyz, view_back),
        0.01,
    );
    let world_per_pixel = select(
        view_depth * 0.8284271 / max(camera.viewport_size.y, 1.0),
        camera.view_right.w,
        camera.view_up.w > 0.5,
    );
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
    output.context_only = select(1u, 0u, is_primary);
    output.emphasis_match = select(0u, 1u, emphasized);
    output.overlay_flags = product.overlay_flags;
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

// The walk replaces focus, route, and hover treatments so hover can never
// change what the walk shows. Visited, current, and future positions are
// distinguished by brightness and afterglow rather than labels.
fn walk_node_color(
    bits: u32,
    base: vec4<f32>,
    sphere_rgb: vec3<f32>,
    circle: f32,
    distance: f32,
    derivative: f32,
) -> vec4<f32> {
    if ((bits & WALK_MEMBER) == 0u) {
        let grey = dot(sphere_rgb, vec3<f32>(0.30, 0.59, 0.11));
        return vec4<f32>(
            mix(sphere_rgb, vec3<f32>(grey), 0.55),
            base.a * circle * WALK_CONTEXT_OPACITY,
        );
    }
    let glow = walk_glow(bits);
    var intensity = 0.62;
    var halo_strength = 0.10;
    var white = 0.0;
    var body_alpha = 0.88;
    if ((bits & WALK_CURRENT) != 0u) {
        intensity = 1.0;
        halo_strength = 0.55 + 0.45 * glow;
        white = 0.16 + 0.34 * glow;
        body_alpha = 1.0;
    } else if ((bits & WALK_NEXT) != 0u) {
        intensity = 0.72 + 0.28 * glow;
        halo_strength = 0.16 + 0.6 * glow;
        white = 0.3 * glow;
        body_alpha = 0.92 + 0.08 * glow;
    } else if ((bits & WALK_VISITED) != 0u) {
        intensity = 0.9;
        halo_strength = 0.30;
        white = 0.06;
        body_alpha = 1.0;
    }
    let body_rgb = min(
        sphere_rgb * intensity + (vec3<f32>(1.0) - sphere_rgb) * white,
        vec3<f32>(1.0),
    );
    let outside = max(distance - 1.0, 0.0);
    let halo = exp(-outside * outside * 3.2) * (1.0 - circle) * halo_strength;
    let halo_rgb = mix(base.rgb, vec3<f32>(1.0), 0.25 + 0.3 * white);
    var rgb = mix(halo_rgb, body_rgb, circle);
    var alpha = max(body_alpha * circle, halo);
    if ((bits & WALK_CURRENT) != 0u) {
        let ring = smoothstep(1.10 - derivative, 1.14, distance)
            - smoothstep(1.22 - derivative, 1.26, distance);
        rgb = mix(rgb, vec3<f32>(1.0, 0.95, 0.82), ring * 0.9);
        alpha = max(alpha, ring * 0.9);
    }
    return vec4<f32>(rgb, alpha);
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
    let source_active = (input.overlay_flags & 1u) != 0u;
    let source_member = (input.overlay_flags & 2u) != 0u;
    let selected = (input.flags & 8192u) != 0u || (input.overlay_flags & 4u) != 0u;
    let neighbor = (input.flags & 16384u) != 0u;
    let route = (input.flags & 32768u) != 0u;
    let walk_active = (input.overlay_flags & WALK_ACTIVE) != 0u;
    let walking_member = walk_lit(input.overlay_flags);
    if (circle <= 0.001 && aura <= 0.001 && !hovered && !selected && !walking_member
        && reader_glow(input.overlay_flags) <= 0.0) {
        discard;
    }

    var color = input.color;
    if (input.context_only != 0u) {
        color.a *= 0.30;
    }
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
    if (walk_active) {
        return walk_node_color(input.overlay_flags, input.color, sphere_rgb, circle, distance, derivative);
    }
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
    if (source_active) {
        // Scope comes from stored provenance; in-scope nodes keep normal
        // rendering even while the anchor is focused.
        if (!source_member && !hovered) {
            color.a *= SOURCE_GHOST_NODE_OPACITY;
        }
    } else if (lens.focus_active != 0u && !hovered && !selected && !neighbor && !route) {
        color.a *= lens.dimmed_node_opacity;
    } else if (lens.focus_active == 0u && lens.topology_emphasis != 0u
        && input.emphasis_match == 0u && !hovered && !selected && !neighbor && !route) {
        color.a *= 0.58;
    }
    if ((input.overlay_flags & STORY_UNTIMED) != 0u && !hovered && !selected) {
        // Untimed: present but neutral, never placed in time.
        let gray = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
        color = vec4<f32>(mix(vec3<f32>(gray), color.rgb, 0.18), color.a * 0.42);
    }
    let spoken = reader_glow(input.overlay_flags);
    if (spoken > 0.0) {
        // Additive only: brighten the body and add a halo; never dim.
        let strength = spoken * select(1.0, 0.3, source_active && !source_member);
        let outside = max(distance - 1.0, 0.0);
        let halo = exp(-outside * outside * 1.4) * (1.0 - circle) * 0.85 * strength;
        let body = min(color.rgb + (vec3<f32>(1.0) - color.rgb) * 0.3 * strength, vec3<f32>(1.0));
        let halo_rgb = mix(input.color.rgb, vec3<f32>(1.0), 0.3);
        color = vec4<f32>(mix(halo_rgb, body, circle), max(color.a, halo));
    }
    return color;
}
