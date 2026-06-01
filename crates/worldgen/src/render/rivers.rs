use image::{Rgba, RgbaImage};

use crate::generate::smoothstep;
use crate::{Surface, Tile, World};

use super::shading::lerp_rgba;

// Lake-spill outlet tiles below this discharge aren't drawn as channels.
const SPILL_MIN: f32 = 0.04;

/// Draw the river network as teal channels.
///
/// Every channel tile draws one tapered segment from its own centre to its
/// downstream neighbour's centre. Because each tile draws its outgoing edge, the
/// whole network — including confluences and lake outlets — connects up without
/// any separate confluence/floodplain/estuary passes.
pub(super) fn draw_rivers(image: &mut RgbaImage, world: &World, scale: u32) {
    // Accumulate channel coverage into a buffer using `max` rather than alpha-
    // compositing each segment in turn. Overlapping/parallel/zigzagging segments
    // then form a solid union instead of aliasing into a hatched "comb", and a
    // stronger channel wins over a weaker one where they cross.
    let width = image.width();
    let height = image.height();
    let mut canvas = ChannelCanvas::new(width as usize, height as usize);

    for idx in 0..world.tiles.len() {
        let tile = &world.tiles[idx];
        if !is_channel(tile) {
            continue;
        }

        let flow = channel_flow(tile);
        let half_width = channel_half_width(tile, flow, scale as f32);
        let color = channel_color(tile, flow);
        let alpha = channel_alpha(tile, flow);
        let start = tile_center(world, idx, scale);

        match downstream_idx(world, idx) {
            Some(nidx) => {
                let end = tile_center(world, nidx, scale);
                let end_half = if world.tiles[nidx].surface == Surface::Ocean {
                    half_width * 1.8 // flare into an estuary at the coast
                } else {
                    half_width
                };
                canvas.stroke(start, end, half_width, end_half, color, alpha);
            }
            // Inland sink: a dot so the headwater/terminus does not vanish.
            None => canvas.stroke(start, start, half_width, half_width, color, alpha),
        }
    }

    canvas.composite(image);
}

fn is_channel(tile: &Tile) -> bool {
    // Only tiles the hydrology classified as an actual channel (order >= 1) or a
    // lake outlet are drawn — not every hillslope tile carrying a trace of sheet
    // flow, which would render as a herringbone of parallel diagonal strokes.
    tile.surface != Surface::Ocean
        && tile.lake_depth <= 0.0
        && (tile.river_order >= 1 || tile.spill_discharge > SPILL_MIN)
}

fn channel_flow(tile: &Tile) -> f32 {
    tile.river.max(tile.spill_discharge * 0.85).clamp(0.0, 1.0)
}

/// Channel order drives width and colour. Spill-only outlet tiles have no order,
/// so fall back to their flow magnitude.
fn order_fraction(tile: &Tile, flow: f32) -> f32 {
    if tile.river_order > 0 {
        ((tile.river_order.min(4) as f32 - 1.0) / 3.0).clamp(0.0, 1.0)
    } else {
        smoothstep(0.05, 0.50, flow)
    }
}

fn channel_half_width(tile: &Tile, flow: f32, scale: f32) -> f32 {
    let order = order_fraction(tile, flow);
    let width = tile.river_width.clamp(0.0, 1.0);
    // Keep a ~1px solid core even on the thinnest channel so segments form a solid
    // ribbon instead of a herringbone of partial-coverage anti-aliased pixels.
    (scale * (0.26 + order * 0.36 + width * 0.18)).max(1.05)
}

fn channel_color(tile: &Tile, flow: f32) -> Rgba<u8> {
    // Teal that reads as water against green/tan terrain; larger/deeper = darker.
    let shallow = Rgba([86, 144, 156, 255]);
    let deep = Rgba([40, 92, 116, 255]);
    let depth =
        (order_fraction(tile, flow) * 0.6 + tile.river_depth.clamp(0.0, 1.0) * 0.4).clamp(0.0, 1.0);
    lerp_rgba(shallow, deep, depth)
}

fn channel_alpha(tile: &Tile, flow: f32) -> f32 {
    (0.55 + order_fraction(tile, flow) * 0.30 + flow * 0.10).clamp(0.0, 0.92)
}

fn tile_center(world: &World, idx: usize, scale: u32) -> (f32, f32) {
    let (x, y) = world.coords(idx);
    let s = scale as f32;
    ((x as f32 + 0.5) * s, (y as f32 + 0.5) * s)
}

fn downstream_idx(world: &World, idx: usize) -> Option<usize> {
    let (dx, dy) = flow_offset(world.tiles[idx].flow_direction)?;
    let (x, y) = world.coords(idx);
    let nx = x as isize + dx as isize;
    let ny = y as isize + dy as isize;
    world
        .in_bounds(nx, ny)
        .then(|| world.idx(nx as usize, ny as usize))
}

fn flow_offset(direction: i8) -> Option<(i32, i32)> {
    match direction {
        0 => Some((0, -1)),
        1 => Some((1, -1)),
        2 => Some((1, 0)),
        3 => Some((1, 1)),
        4 => Some((0, 1)),
        5 => Some((-1, 1)),
        6 => Some((-1, 0)),
        7 => Some((-1, -1)),
        _ => None,
    }
}

/// A scratch buffer holding, per pixel, the strongest channel coverage (already
/// scaled by the channel's opacity) and that channel's colour. Strokes are
/// combined with `max`, then composited onto the image in a single pass.
struct ChannelCanvas {
    width: usize,
    height: usize,
    coverage: Vec<f32>,
    color: Vec<[u8; 3]>,
}

impl ChannelCanvas {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            coverage: vec![0.0; width * height],
            color: vec![[0, 0, 0]; width * height],
        }
    }

    /// Rasterise a tapered segment: each pixel within the (linearly interpolated)
    /// half-width of the centre line takes `coverage * opacity`, with a ~1px
    /// feather for a soft edge. A zero-length segment renders as a single dot.
    fn stroke(
        &mut self,
        (x0, y0): (f32, f32),
        (x1, y1): (f32, f32),
        half0: f32,
        half1: f32,
        color: Rgba<u8>,
        opacity: f32,
    ) {
        let max_half = half0.max(half1) + 1.0;
        let lo_x = (x0.min(x1) - max_half).floor().max(0.0) as usize;
        let hi_x = ((x0.max(x1) + max_half).ceil() as usize).min(self.width - 1);
        let lo_y = (y0.min(y1) - max_half).floor().max(0.0) as usize;
        let hi_y = ((y0.max(y1) + max_half).ceil() as usize).min(self.height - 1);
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let rgb = [color[0], color[1], color[2]];

        for py in lo_y..=hi_y {
            for px in lo_x..=hi_x {
                let fx = px as f32 + 0.5;
                let fy = py as f32 + 0.5;
                let t = (((fx - x0) * dx + (fy - y0) * dy) / len2).clamp(0.0, 1.0);
                let cx = x0 + dx * t;
                let cy = y0 + dy * t;
                let dist = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
                let half = half0 + (half1 - half0) * t;
                // Solid core with a 1px linear feather at the rim.
                let value = (half + 0.5 - dist).clamp(0.0, 1.0) * opacity;
                let i = py * self.width + px;
                if value > self.coverage[i] {
                    self.coverage[i] = value;
                    self.color[i] = rgb;
                }
            }
        }
    }

    fn composite(&self, image: &mut RgbaImage) {
        for y in 0..self.height {
            for x in 0..self.width {
                let i = y * self.width + x;
                let a = self.coverage[i];
                if a <= 0.0 {
                    continue;
                }
                let base = image.get_pixel(x as u32, y as u32).0;
                let c = self.color[i];
                let blend = |b: u8, c: u8| (b as f32 * (1.0 - a) + c as f32 * a).round() as u8;
                image.put_pixel(
                    x as u32,
                    y as u32,
                    Rgba([
                        blend(base[0], c[0]),
                        blend(base[1], c[1]),
                        blend(base[2], c[2]),
                        255,
                    ]),
                );
            }
        }
    }
}
