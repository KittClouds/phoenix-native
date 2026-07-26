struct CameraUniform {
    view_proj: mat4x4<f32>,
    eye_position: vec4<f32>,
    view_right: vec4<f32>,
    view_up: vec4<f32>,
    viewport_size: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let position = positions[vertex_index];
    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.9999, 1.0);
    output.uv = position * 0.5 + vec2<f32>(0.5);
    return output;
}

fn line_grid(point: vec2<f32>, scale: f32) -> f32 {
    let cell = fract(point * scale);
    let nearest = min(cell, vec2<f32>(1.0) - cell);
    let distance = min(nearest.x, nearest.y);
    return 1.0 - smoothstep(0.0, 0.018, distance);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let viewport = max(camera.viewport_size, vec2<f32>(1.0));
    var point = input.uv * 2.0 - vec2<f32>(1.0);
    point.x *= viewport.x / viewport.y;

    let upper_left = point - vec2<f32>(-0.72, -0.42);
    let lower_right = point - vec2<f32>(0.82, 0.56);
    let center = point - vec2<f32>(0.05, 0.02);
    let phthalo = exp(-dot(upper_left * vec2<f32>(0.72, 1.18), upper_left) * 1.7);
    let cyan = exp(-dot(lower_right * vec2<f32>(0.82, 1.35), lower_right) * 2.3);
    let atmosphere = exp(-dot(center * vec2<f32>(0.48, 0.82), center) * 1.1);

    var color = vec3<f32>(0.007, 0.012, 0.014);
    color += vec3<f32>(0.010, 0.090, 0.071) * phthalo;
    color += vec3<f32>(0.010, 0.048, 0.061) * cyan;
    color += vec3<f32>(0.008, 0.025, 0.024) * atmosphere;

    let major_grid = line_grid(point, 2.25);
    let minor_grid = line_grid(point, 9.0);
    color += vec3<f32>(0.055, 0.145, 0.122) * major_grid * 0.11;
    color += vec3<f32>(0.035, 0.095, 0.082) * minor_grid * 0.045;

    let vignette_radius = dot(point * vec2<f32>(0.72, 0.92), point);
    let vignette = 1.0 - smoothstep(0.42, 2.15, vignette_radius);
    color *= mix(0.58, 1.0, vignette);

    return vec4<f32>(color, 1.0);
}
