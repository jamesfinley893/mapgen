use crate::generate::util::{hash01, normalize};

// Returns angle offset (radians) from pure westerly. Varies per seed so each world
// has a distinct prevailing wind direction (+/-PI/4 range).
pub(super) fn prevailing_wind_angle(seed: u64) -> f32 {
    (hash01(seed, 0x7135_DE00, 1) - 0.5) * std::f32::consts::FRAC_PI_2
}

// Hadley cell model: tropical easterlies, mid-latitude westerlies, polar easterlies.
// world_tilt rotates the whole pattern so each seed has a distinct slant.
pub(super) fn wind_at_latitude(world_tilt: f32, lat: f32) -> (f32, f32) {
    let zonal = if !(0.24..=0.70).contains(&lat) {
        -1.0_f32 // easterlies
    } else {
        1.0_f32 // westerlies
    };
    let (s, c) = world_tilt.sin_cos();
    normalize((zonal * c, zonal * s))
}
