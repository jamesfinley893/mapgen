use crate::generate::util::smoothstep;

pub(super) fn normalize_terrain(terrain: &mut [f32], low_q: f32, high_q: f32) {
    let mut sorted = terrain.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let last = sorted.len().saturating_sub(1);
    let lo_idx = ((last as f32) * low_q).round() as usize;
    let hi_idx = ((last as f32) * high_q).round() as usize;
    let lo = sorted[lo_idx.min(last)];
    let hi = sorted[hi_idx.min(last)].max(lo + 0.0001);

    for value in terrain.iter_mut() {
        let mapped = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
        let compressed = smoothstep(0.0, 1.0, mapped).powf(1.04);
        let top_tail = smoothstep(0.80, 1.0, compressed);
        *value = (compressed - top_tail * 0.05).clamp(0.0, 1.0);
    }
}
