use noise::OpenSimplex;

use crate::World;
use crate::generate::util::{octave_noise, ridge_noise, smoothstep};

use super::fields::OrogenFields;

pub(super) fn apply_mountain_crag_detail(
    world: &World,
    ridge: &OpenSimplex,
    fields: &OrogenFields,
    terrain: &mut [f32],
) {
    let ws = world.effective_world_size();
    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let current = terrain[idx];
            if current <= world.sea_level {
                continue;
            }

            let uplift = smoothstep(
                0.10,
                0.58,
                fields.axial_uplift[idx] + fields.shoulder_uplift[idx] * 0.65,
            );
            let highland = smoothstep(0.58, 0.86, current);
            let crag_mask = highland * (0.35 + uplift * 0.65);
            if crag_mask <= 0.0 {
                continue;
            }

            let xf = x as f64 / ws as f64;
            let yf = y as f64 / ws as f64;
            let ribs = ridge_noise(ridge, xf * 18.0 + 7.0, yf * 18.0 - 11.0, 3);
            let fracture = octave_noise(ridge, xf * 31.0 - 19.0, yf * 31.0 + 23.0, 2, 0.52, 2.1);
            let detail = (ribs - 0.40) * 0.070 + (fracture - 0.5) * 0.032;
            terrain[idx] = (current + detail * crag_mask).max(0.02);
        }
    }
}
