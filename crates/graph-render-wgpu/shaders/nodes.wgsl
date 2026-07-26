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

struct NodeProductGpu {
    family_mask: vec2<u32>,
    scope_mask: vec2<u32>,
    review_mask: u32,
    enabled: u32,
    _padding: vec2<u32>,
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
@group(2) @binding(0) var<uniform> lens: GraphLensUniform;
@group(2) @binding(1) var<storage, read> node_products: array<NodeProductGpu>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) flags: u32,
    @location(3) @interpolate(flat) visible: u32,
};

fn intersects(left: vec2<u32>, right: vec2<u32>) -> bool {
    return ((left.x & right.x) | (left.y & right.y)) != 0u;
}

fn node_visible(product: NodeProductGpu) -> bool {
    if (lens.product_index_enabled == 0u) {
        return true;
    }
    return product.enabled != 0u
        && intersects(product.family_mask, lens.family_mask)
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
    let uv = corners[vertex_index] * 1.3;
    let world = node.position_radius.xyz
        + (camera.view_right.xyz * uv.x + camera.view_up.xyz * uv.y) * node.position_radius.w;

    var output: VertexOutput;
    output.position = select(
        vec4<f32>(2.0, 2.0, 2.0, 1.0),
        camera.view_proj * vec4<f32>(world, 1.0),
        is_visible,
    );
    output.uv = uv;
    output.color = node.color;
    output.flags = node.kind_flags & 0xffffu;
    output.visible = select(0u, 1u, is_visible);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.visible == 0u) {
        discard;
    }
    let distance = length(input.uv);
    let derivative = max(fwidth(distance), 0.0001);
    let circle = 1.0 - smoothstep(1.0 - derivative, 1.0 + derivative, distance);
    let hovered = (input.flags & 1u) != 0u;
    let selected = (input.flags & 2u) != 0u;
    if (circle <= 0.001 && !hovered && !selected) {
        discard;
    }

    var color = input.color;
    if (selected) {
        let ring = smoothstep(0.96 - derivative, 1.0, distance)
            - smoothstep(1.20 - derivative, 1.24, distance);
        color = mix(color, vec4<f32>(1.0, 0.78, 0.16, 1.0), ring);
    } else if (hovered) {
        let ring = smoothstep(0.97 - derivative, 1.0, distance)
            - smoothstep(1.10 - derivative, 1.14, distance);
        color = mix(color, vec4<f32>(0.15, 0.95, 0.88, 1.0), ring);
    }
    color.a *= circle;
    return color;
}
