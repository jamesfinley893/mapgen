use std::collections::HashMap;

use worldgen::{Surface, World, WorldConfig, build_metadata, generate_world};

fn main() -> Result<(), String> {
    let seeds = [42_u64, 97, 3000, 6564451133730440783, 14407461047914957368];

    for seed in seeds {
        let config = WorldConfig {
            seed,
            width: 256,
            height: 256,
            render_scale: 2,
            ..WorldConfig::default()
        };
        let world = generate_world(&config)?;
        let metadata = build_metadata(&world, &config);
        let audit = audit_rivers(&world);

        println!("seed {seed}");
        println!(
            "  river_tiles={} river_land_pct={:.2} bands={:?} longest_trunk={} straight={:.3} max_q={:.2} max_power={:.3}",
            metadata.river_tiles,
            metadata.river_tiles as f32 / metadata.land_tiles.max(1) as f32 * 100.0,
            metadata.river_band_counts,
            metadata.longest_trunk_length,
            metadata.trunk_straight_run_ratio,
            metadata.max_river_discharge,
            metadata.max_stream_power
        );
        println!(
            "  sources={} confluences={} mouths={} branches_per_mouth={:.1}",
            audit.sources,
            audit.confluences,
            audit.mouths,
            audit.sources as f32 / audit.mouths.max(1) as f32
        );
        println!(
            "  segment_len min={} median={} p90={} max={}",
            audit.segment_min, audit.segment_median, audit.segment_p90, audit.segment_max
        );
        println!(
            "  direction cardinal={:.2}% diagonal={:.2}% dominant={:?}:{:.2}%",
            audit.cardinal_pct, audit.diagonal_pct, audit.dominant_dir, audit.dominant_pct
        );
    }

    Ok(())
}

struct RiverAudit {
    sources: usize,
    confluences: usize,
    mouths: usize,
    segment_min: usize,
    segment_median: usize,
    segment_p90: usize,
    segment_max: usize,
    cardinal_pct: f32,
    diagonal_pct: f32,
    dominant_dir: (isize, isize),
    dominant_pct: f32,
}

fn audit_rivers(world: &World) -> RiverAudit {
    let mut upstream = vec![0_usize; world.tiles.len()];
    for tile in &world.tiles {
        if tile.surface != Surface::River {
            continue;
        }
        if let Some(next) = tile.downstream {
            if world.tiles[next].surface == Surface::River {
                upstream[next] += 1;
            }
        }
    }

    let mut sources = 0;
    let mut confluences = 0;
    let mut mouths = 0;
    let mut segment_lengths = Vec::new();
    let mut direction_counts = HashMap::<(isize, isize), usize>::new();
    let mut cardinal = 0_usize;
    let mut diagonal = 0_usize;
    let mut steps = 0_usize;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River {
            continue;
        }
        if upstream[idx] == 0 {
            sources += 1;
            segment_lengths.push(path_len_to_junction_or_sink(world, &upstream, idx));
        } else if upstream[idx] > 1 {
            confluences += 1;
        }

        match tile.downstream {
            Some(next) if world.tiles[next].surface == Surface::River => {
                let dir = direction(world, idx, next);
                *direction_counts.entry(dir).or_default() += 1;
                if dir.0 == 0 || dir.1 == 0 {
                    cardinal += 1;
                } else {
                    diagonal += 1;
                }
                steps += 1;
            }
            _ => mouths += 1,
        }
    }

    segment_lengths.sort_unstable();
    let segment_min = segment_lengths.first().copied().unwrap_or(0);
    let segment_median = percentile(&segment_lengths, 0.50);
    let segment_p90 = percentile(&segment_lengths, 0.90);
    let segment_max = segment_lengths.last().copied().unwrap_or(0);
    let ((dominant_dir, dominant_count), _) = direction_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|entry| (entry, ()))
        .unwrap_or((((0, 0), 0), ()));

    RiverAudit {
        sources,
        confluences,
        mouths,
        segment_min,
        segment_median,
        segment_p90,
        segment_max,
        cardinal_pct: cardinal as f32 / steps.max(1) as f32 * 100.0,
        diagonal_pct: diagonal as f32 / steps.max(1) as f32 * 100.0,
        dominant_dir,
        dominant_pct: dominant_count as f32 / steps.max(1) as f32 * 100.0,
    }
}

fn path_len_to_junction_or_sink(world: &World, upstream: &[usize], start: usize) -> usize {
    let mut len = 0;
    let mut current = start;
    let mut guard = 0;
    while guard < world.tiles.len() {
        len += 1;
        let Some(next) = world.tiles[current].downstream else {
            break;
        };
        if world.tiles[next].surface != Surface::River || upstream[next] > 1 {
            break;
        }
        current = next;
        guard += 1;
    }
    len
}

fn direction(world: &World, idx: usize, next: usize) -> (isize, isize) {
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    (
        (nx as isize - x as isize).signum(),
        (ny as isize - y as isize).signum(),
    )
}

fn percentile(values: &[usize], p: f32) -> usize {
    if values.is_empty() {
        return 0;
    }
    let idx = ((values.len() - 1) as f32 * p).round() as usize;
    values[idx]
}
