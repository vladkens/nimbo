// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Metal renderer's shaders: rounded rects with borders, shadows and gradients, thick lines,
//! atlas glyphs and images, each the twin of its function in `src/cpu/raster.rs` (the names are
//! the ones it cites), so both renderers draw the same pixels; change them together.
//!
//! Every pipeline writes premultiplied colour, blended with (One, OneMinusSourceAlpha) into
//! `BGRA8Unorm`, in sRGB space. Coverage is analytic: a signed distance in points times the
//! scale gives pixels, and a pixel is covered by `0.5 - distance`. Per-corner radii and per-side
//! borders follow the approach of Zed's quad shader (`crates/gpui_apple/src/shaders.metal`,
//! Apache-2.0), simplified; no code of it is copied.

#include <metal_stdlib>
using namespace metal;

struct Globals {
  float2 target; // the target's size in whole pixels
  float scale;   // pixels per point
  float _pad;
};

struct Quad {
  float4 rect; // x, y, w, h in points
  float4 color;
  float4 border_color;
  float4 radii;   // top-left, top-right, bottom-right, bottom-left
  float4 borders; // top, right, bottom, left
  float blur;     // > 0: draw only a soft shadow of the shape
  float gradient; // 1: color to border_color down the rect, 2: across it
  float _pad0, _pad1;
};

struct Sprite {
  float4 rect; // on screen, points
  float4 uv;   // in the texture, pixels
  float4 color;
};

struct Segment {
  float2 a;
  float2 b;
  float4 color;
  float width;
  float round; // 1: round caps, 0: butt caps
  float _pad0, _pad1;
};

constant float2 corners[4] = {{0, 0}, {1, 0}, {0, 1}, {1, 1}};

// A point to clip space through the target's whole pixels: the CPU renderer draws a fractional
// size truncated to them, at the same pixel centres.
float4 to_clip(float2 p, constant Globals &g) {
  return float4(p * g.scale / g.target * float2(2, -2) + float2(-1, 1), 0, 1);
}

// What a fragment writes: an opaque colour already rounded to 8 bits, ties to even as `raster.rs`'s
// `unorm`; the store rounds otherwise, as through half precision (0.9 is 229.5 on the CPU, which
// stores 230, and the GPU stored 229).
float4 store(float4 c) {
  return c.a >= 1.0 ? rint(saturate(c) * 255.0) / 255.0 : c;
}

// A straight sRGB colour to premultiplied.
float4 premul(float4 c) {
  return float4(c.rgb * c.a, c.a);
}

// MARK: Quads

struct QuadOut {
  float4 position [[position]];
  float4 fill [[flat]];
  float4 border [[flat]];
  uint id [[flat]];
};

// The rect padded by the blur and a pixel, as `quad_bounds`; nothing for a rect without
// positive width and height (an intersection that missed), which the padding would turn into a
// faint stripe.
vertex QuadOut quad_vs(uint vid [[vertex_id]], uint iid [[instance_id]],
                       constant Globals &g [[buffer(0)]], const device Quad *quads [[buffer(1)]]) {
  Quad q = quads[iid];
  float pad = q.blur * 2.0 + 1.0 / g.scale;
  bool empty = !(q.rect.z > 0.0 && q.rect.w > 0.0);
  float2 p = q.rect.xy - pad + corners[vid] * (empty ? 0.0 : q.rect.zw + 2.0 * pad);
  return {to_clip(p, g), premul(q.color), premul(q.border_color), iid};
}

// The radius of the corner in the quadrant of `c` (a point relative to the centre).
float pick_radius(float2 c, float4 radii) {
  return c.x < 0.0 ? (c.y < 0.0 ? radii.x : radii.w) : (c.y < 0.0 ? radii.y : radii.z);
}

// Signed distance to a rounded rect with the given corner radius: negative inside. `c` is the
// point relative to the centre.
float rounded_rect(float2 c, float2 half_size, float r) {
  float2 q = abs(c) - half_size + r;
  return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

// `shade_quad`.
fragment float4 quad_fs(QuadOut in [[stage_in]], constant Globals &g [[buffer(0)]],
                        const device Quad *quads [[buffer(1)]]) {
  Quad q = quads[in.id];
  // The pixel centre in points, as `raster.rs`'s `centre` (an interpolated point is a bit off).
  float2 p = in.position.xy / g.scale;
  float2 half_size = q.rect.zw * 0.5;
  float2 c = p - (q.rect.xy + half_size);
  float4 fill = in.fill;
  if (q.gradient > 0.0) {
    float t = q.gradient == 1.0 ? (p.y - q.rect.y) / q.rect.w : (p.x - q.rect.x) / q.rect.z;
    fill = mix(in.fill, in.border, saturate(t));
  }

  if (q.blur == 0.0 && all(q.radii == 0.0) && all(q.borders == 0.0)) {
    // A plain rect, the most common quad (backgrounds, panels, selections).
    float2 e = abs(c) - half_size;
    return store(fill * saturate(0.5 - max(e.x, e.y) * g.scale));
  }

  float r = min(pick_radius(c, q.radii), min(half_size.x, half_size.y));
  float d = rounded_rect(c, half_size, r);
  if (q.blur > 0.0) {
    return store(fill * (1.0 - smoothstep(-q.blur, q.blur, d)));
  }

  float outer = saturate(0.5 - d * g.scale);
  if (all(q.borders == 0.0)) {
    return store(fill * outer);
  }

  // The inner edge: the rect inset by each side's border; a side without a border moves out
  // by a pixel so it shows no border colour. The inner corner is the one of the quadrant around
  // the inner centre (uneven borders move it off the outer one), a circle of its outer radius
  // less the wider of its two borders (exact for equal borders).
  float4 b = select(q.borders, float4(-1.0 / g.scale), q.borders == 0.0);
  float2 lo = q.rect.xy + b.wx;
  float2 hi = q.rect.xy + q.rect.zw - b.yz;
  float2 inner_half = max((hi - lo) * 0.5, 0.0);
  float2 ci = p - (lo + hi) * 0.5;
  float ro = min(pick_radius(ci, q.radii), min(half_size.x, half_size.y));
  float2 side = float2(ci.x < 0.0 ? b.w : b.y, ci.y < 0.0 ? b.x : b.z);
  float ri = min(max(ro - max(side.x, side.y), 0.0), min(inner_half.x, inner_half.y));
  // Borders that meet or cross leave no fill.
  float inner = 0.0;
  if (hi.x > lo.x && hi.y > lo.y) {
    inner = saturate(0.5 - rounded_rect(ci, inner_half, ri) * g.scale);
  }

  return store(mix(in.border, fill, inner) * outer);
}

// MARK: Lines

struct LineOut {
  float4 position [[position]];
  float4 color [[flat]];
  uint id [[flat]];
};

// The segment's rotated quad, as `line_bounds` bounds it; nothing for a butt-capped one of no
// length, which has no area (a round-capped one is a dot).
vertex LineOut line_vs(uint vid [[vertex_id]], uint iid [[instance_id]],
                       constant Globals &g [[buffer(0)]],
                       const device Segment *segs [[buffer(1)]]) {
  Segment s = segs[iid];
  float2 ab = s.b - s.a;
  float len = length(ab);
  float2 dir = len > 0.0 ? ab / len : float2(1, 0);
  float2 normal = float2(-dir.y, dir.x);
  // Half the drawn width (at least a pixel wide) plus a pixel for the antialiased edge.
  float ext = max(s.width, 1.0 / g.scale) * 0.5 + 1.0 / g.scale;
  if (s.round <= 0.0 && all(s.a == s.b)) {
    ext = 0.0;
  }

  float2 k = corners[vid];
  float2 p = mix(s.a - dir * ext, s.b + dir * ext, k.x) + normal * (k.y * 2.0 - 1.0) * ext;
  return {to_clip(p, g), premul(s.color), iid};
}

// `shade_line`.
fragment float4 line_fs(LineOut in [[stage_in]], constant Globals &g [[buffer(0)]],
                        const device Segment *segs [[buffer(1)]]) {
  Segment s = segs[in.id];
  // Lines thinner than a pixel draw a pixel wide with the alpha scaled down.
  float px = 1.0 / g.scale;
  float w = max(s.width, px);
  float faint = min(s.width / px, 1.0);
  float2 pa = in.position.xy / g.scale - s.a;
  float2 ab = s.b - s.a;
  float len2 = dot(ab, ab);
  float d;
  if (s.round > 0.0) {
    float h = len2 > 0.0 ? saturate(dot(pa, ab) / len2) : 0.0;
    d = length(pa - ab * h) - w * 0.5;
  } else {
    float len = sqrt(len2);
    float2 dir = len > 0.0 ? ab / len : float2(1, 0);
    float t = dot(pa, dir);
    float n = abs(dot(pa, float2(-dir.y, dir.x)));
    d = max(max(-t, t - len), n - w * 0.5);
  }

  return store(in.color * (saturate(0.5 - d * g.scale) * faint));
}

// MARK: Sprites

struct SpriteOut {
  float4 position [[position]];
  float2 uv;
  float4 color [[flat]];
};

vertex SpriteOut sprite_vs(uint vid [[vertex_id]], uint iid [[instance_id]],
                           constant Globals &g [[buffer(0)]],
                           const device Sprite *sprites [[buffer(1)]]) {
  Sprite s = sprites[iid];
  float2 c = corners[vid];
  return {to_clip(s.rect.xy + c * s.rect.zw, g), s.uv.xy + c * s.uv.zw, premul(s.color)};
}

// `glyph`: coverage from the R8 atlas, nearest texel, times the colour.
fragment float4 glyph_fs(SpriteOut in [[stage_in]], texture2d<float> atlas [[texture(0)]]) {
  constexpr sampler nearest(coord::pixel, filter::nearest, address::clamp_to_edge);
  return store(in.color * atlas.sample(nearest, in.uv).r);
}

// `image`: premultiplied RGBA, linear filtering (`sample`), times the premultiplied tint.
fragment float4 image_fs(SpriteOut in [[stage_in]], texture2d<float> tex [[texture(0)]]) {
  constexpr sampler linear(coord::pixel, filter::linear, address::clamp_to_edge);
  return store(tex.sample(linear, in.uv) * in.color);
}
