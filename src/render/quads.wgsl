struct Globals {
    screen: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> globals: Globals;

struct InstanceIn {
    @location(0) position: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    // Top-left, top-right, bottom-right, bottom-left.
    @location(3) radii: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) radii: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32, instance: InstanceIn) -> VertexOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );

    let corner = corners[vertex_index];
    let pixel = instance.position + corner * instance.size;
    let ndc = vec2<f32>(
        pixel.x / globals.screen.x * 2.0 - 1.0,
        1.0 - pixel.y / globals.screen.y * 2.0,
    );

    var out: VertexOut;
    out.clip_position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = instance.color;
    out.local = corner;
    out.size = instance.size;
    out.radii = instance.radii;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    if (max(max(in.radii.x, in.radii.y), max(in.radii.z, in.radii.w)) <= 0.0) {
        return in.color;
    }

    let point = (in.local - vec2<f32>(0.5)) * in.size;
    let half_size = in.size * 0.5;

    var radius = in.radii.x;
    if (point.x > 0.0 && point.y <= 0.0) {
        radius = in.radii.y;
    } else if (point.x > 0.0 && point.y > 0.0) {
        radius = in.radii.z;
    } else if (point.x <= 0.0 && point.y > 0.0) {
        radius = in.radii.w;
    }
    radius = clamp(radius, 0.0, min(half_size.x, half_size.y));

    let q = abs(point) - half_size + vec2<f32>(radius);
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    let antialias = max(fwidth(distance), 0.001);
    let alpha = 1.0 - smoothstep(-antialias, antialias, distance);
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
