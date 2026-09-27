struct CameraUniform {
    view_proj: mat4x4<f32>,
    eye_position: vec4<f32>,
    view_right: vec4<f32>,
    view_up: vec4<f32>,
    viewport_size: vec2<f32>,
    edge_opacity: f32,
    canvas_style: f32,
};

struct ParticleGpu {
    position_size: vec4<f32>,
    color: vec4<f32>,
    offset: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(1) @binding(0) var<storage, read> particles: array<ParticleGpu>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

// Route-walk particles are screen-space glows anchored to world positions.
// Radius and offset are physical pixels so the stream reads the same at any
// zoom level.
@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let particle = particles[instance_index];
    let corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0,  1.0),
    );
    let corner = corners[vertex_index];
    let clip = camera.view_proj * vec4<f32>(particle.position_size.xyz, 1.0);
    // The glow quad is wider than the core so the soft halo is not clipped.
    let pixels = corner * particle.position_size.w * 3.2 + particle.offset.xy;
    let ndc_offset = pixels * 2.0 / max(camera.viewport_size, vec2<f32>(1.0, 1.0));
    var output: VertexOutput;
    output.position = select(
        vec4<f32>(2.0, 2.0, 2.0, 1.0),
        vec4<f32>(clip.xy + ndc_offset * clip.w, clip.z, clip.w),
        clip.w > 0.0,
    );
    output.uv = corner * 3.2;
    output.color = particle.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let d2 = dot(input.uv, input.uv);
    let core = exp(-d2 * 2.4);
    let halo = exp(-d2 * 0.42) * 0.38;
    let strength = (core + halo) * input.color.a;
    if (strength < 0.002) {
        discard;
    }
    let rgb = mix(input.color.rgb, vec3<f32>(1.0, 1.0, 1.0), core * 0.55);
    return vec4<f32>(rgb, strength);
}
