use worldgen::{WorldConfig, audit_rivers, build_metadata, generate_world};

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
            audit.cardinal_fraction * 100.0,
            audit.diagonal_fraction * 100.0,
            audit.dominant_direction,
            audit.dominant_direction_fraction * 100.0
        );
    }

    Ok(())
}
