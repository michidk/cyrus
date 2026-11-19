import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_render::color_operations::linear_to_srgb;
import package::glide_fog::{framebuffer_blend_integer, framebuffer_dither};

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var artwork: texture_2d<f32>;
@group(0) @binding(2) var text: texture_2d<f32>;

fn layer(destination: vec3<f32>, source: vec4<f32>, pixel: vec2<u32>) -> vec3<f32> {
    let alpha=u32(round(source.a*255.0));
    if alpha==0u { return destination; }
    if alpha==255u { return source.rgb; }
    return framebuffer_dither(framebuffer_blend_integer(destination,linear_to_srgb(source.rgb),alpha,pixel),pixel);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let actual=vec2<i32>(in.position.xy);
    let size=vec2<f32>(textureDimensions(screen));
    let viewport=min(size.x/640.0,size.y/480.0)*vec2(640.0,480.0);
    let position=(in.position.xy-(size-viewport)*0.5)*vec2(640.0,480.0)/viewport;
    let destination=textureLoad(screen,actual,0);
    if any(position<vec2(0.0)) || any(position>=vec2(640.0,480.0)) { return destination; }
    let pixel=vec2<u32>(position);
    let base=textureLoad(artwork,vec2<i32>(pixel),0);
    let foreground=textureLoad(text,vec2<i32>(pixel),0);
    return vec4(layer(layer(destination.rgb,base,pixel),foreground,pixel),destination.a);
}
