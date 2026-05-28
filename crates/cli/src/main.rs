use std::fs;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use rand::random;
use time::OffsetDateTime;
use time::format_description::FormatItem;
use time::macros::format_description;
use worldgen::{RenderConfig, World, WorldConfig, build_metadata, generate_world, render_world};

#[derive(Debug, Parser)]
#[command(name = "mapgen")]
#[command(about = "Generate seeded tile-based world maps")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Re-render a previously exported tiles.json to a PNG for verification.
    Render {
        /// Path to a tiles.json file (or a run directory containing one).
        #[arg(long)]
        input: PathBuf,
    },
    Generate {
        #[arg(long)]
        seed: Option<u64>,
        #[arg(long, default_value_t = 384)]
        width: usize,
        #[arg(long, default_value_t = 384)]
        height: usize,
        /// World scale multiplier. Expands tile dimensions N× while keeping pixels/tile
        /// constant at the 384-base default (~4 px/tile). --scale 2 generates a 768×768
        /// world, not just a zoomed-in 384×384.
        #[arg(long)]
        scale: Option<u32>,
        #[arg(long, default_value_t = 0.52)]
        sea_level: f32,
        #[arg(long, default_value_t = 0.0)]
        temperature_bias: f32,
        #[arg(long, default_value_t = 0.0)]
        moisture_bias: f32,
        #[arg(long, default_value_t = 1.0)]
        rainfall_scale: f32,
        #[arg(long, default_value_t = 1.0)]
        runoff_scale: f32,
        #[arg(long, default_value_t = 1.0)]
        channel_density: f32,
        /// Tiles per world unit. 0 or omit = match min(width, height) for a single world unit.
        /// Set to a fixed value (e.g. 384) to make larger maps cover more geographic area.
        #[arg(long, default_value_t = 0)]
        world_size: u32,
        #[arg(long, default_value = "output")]
        out_dir: PathBuf,
        /// Export full per-tile data as tiles.json alongside the PNG.
        #[arg(long, default_value_t = false)]
        export_tiles: bool,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Render { input } => {
            let tiles_path = if input.is_dir() {
                input.join("tiles.json")
            } else {
                input.clone()
            };
            let run_dir = tiles_path
                .parent()
                .ok_or("tiles.json has no parent directory")?
                .to_path_buf();
            let json = fs::read_to_string(&tiles_path)
                .map_err(|err| format!("failed to read {}: {err}", tiles_path.display()))?;
            let world: World = serde_json::from_str(&json)
                .map_err(|err| format!("failed to parse tiles.json: {err}"))?;
            let render_scale =
                (1536_u32 / world.width.max(world.height) as u32).clamp(1, 32);
            let image = render_world(&world, RenderConfig { scale: render_scale });
            let out_path = run_dir.join("rerendered.png");
            image
                .save(&out_path)
                .map_err(|err| format!("failed to write PNG: {err}"))?;
            println!("wrote {}", out_path.display());
            Ok(())
        }
        Commands::Generate {
            seed,
            width,
            height,
            scale,
            sea_level,
            temperature_bias,
            moisture_bias,
            rainfall_scale,
            runoff_scale,
            channel_density,
            world_size,
            out_dir,
            export_tiles,
        } => {
            let seed = select_seed(seed);
            validate_dimensions(width, height)?;
            // pixels/tile is always derived from the base (unscaled) dimensions so that
            // --scale never changes visual density, only world size.
            let render_scale = (1536_u32 / width.max(height) as u32).clamp(1, 32);
            let (width, height, world_size) = match scale {
                Some(s) if s > 1 => {
                    let w = width.saturating_mul(s as usize).min(4096);
                    let h = height.saturating_mul(s as usize).min(4096);
                    // If world_size was not set explicitly, fix it to the base tile count
                    // so each tile covers the same geographic area at any scale factor.
                    let ws = if world_size == 0 {
                        width.min(height) as u32
                    } else {
                        world_size
                    };
                    (w, h, ws)
                }
                _ => (width, height, world_size),
            };
            let config = WorldConfig {
                seed,
                width,
                height,
                sea_level,
                temperature_bias,
                moisture_bias,
                rainfall_scale,
                runoff_scale,
                channel_density,
                render_scale,
                world_size,
            };
            config.validate()?;

            let world = generate_world(&config)?;
            let image = render_world(
                &world,
                RenderConfig {
                    scale: render_scale,
                },
            );
            let metadata = build_metadata(&world, &config);
            let run_dir = build_run_output_dir(&out_dir, seed, OffsetDateTime::now_utc())?;
            let png_path = run_dir.join("map.png");
            let json_path = run_dir.join("metadata.json");

            fs::create_dir_all(&run_dir)
                .map_err(|err| format!("failed to create output directory: {err}"))?;

            image
                .save(&png_path)
                .map_err(|err| format!("failed to write PNG: {err}"))?;
            let json = serde_json::to_string_pretty(&metadata)
                .map_err(|err| format!("failed to serialize metadata: {err}"))?;
            fs::write(&json_path, json)
                .map_err(|err| format!("failed to write metadata: {err}"))?;
            if export_tiles {
                let tiles_path = run_dir.join("tiles.json");
                let tiles_json = serde_json::to_string(&world)
                    .map_err(|err| format!("failed to serialize tiles: {err}"))?;
                fs::write(&tiles_path, tiles_json)
                    .map_err(|err| format!("failed to write tiles: {err}"))?;
                println!("wrote {}", tiles_path.display());
            }

            println!("seed {}", seed);
            println!("wrote {}", run_dir.display());
            println!("wrote {}", png_path.display());
            println!("wrote {}", json_path.display());
            Ok(())
        }
    }
}

fn select_seed(seed: Option<u64>) -> u64 {
    seed.unwrap_or_else(random::<u64>)
}

fn validate_dimensions(width: usize, height: usize) -> Result<(), String> {
    if width < 32 || height < 32 {
        return Err("width and height must be at least 32".into());
    }
    if width > 4096 || height > 4096 {
        return Err("width and height must be at most 4096".into());
    }
    Ok(())
}

fn build_run_output_dir(
    base: &std::path::Path,
    seed: u64,
    now: OffsetDateTime,
) -> Result<PathBuf, String> {
    Ok(base.join(build_run_dir_name(seed, now)?))
}

fn build_run_dir_name(seed: u64, now: OffsetDateTime) -> Result<String, String> {
    static TIMESTAMP_FORMAT: &[FormatItem<'static>] =
        format_description!("[year][month][day]-[hour][minute][second]Z");
    let timestamp = now
        .format(TIMESTAMP_FORMAT)
        .map_err(|err| format!("failed to format timestamp: {err}"))?;
    Ok(format!("seed-{seed}_{timestamp}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn run_dir_name_uses_seed_and_timestamp() {
        let now = datetime!(2024-06-02 08:34:56 UTC);
        let name = build_run_dir_name(42, now).unwrap();
        assert_eq!(name, "seed-42_20240602-083456Z");
    }

    #[test]
    fn run_output_dir_joins_base_and_generated_name() {
        let now = datetime!(2024-06-02 08:34:56 UTC);
        let path = build_run_output_dir(std::path::Path::new("output/worlds"), 7, now).unwrap();
        assert_eq!(path, PathBuf::from("output/worlds/seed-7_20240602-083456Z"));
    }

    #[test]
    fn select_seed_preserves_explicit_seed() {
        assert_eq!(select_seed(Some(12345)), 12345);
    }

    #[test]
    fn select_seed_generates_random_seed_when_missing() {
        let a = select_seed(None);
        let b = select_seed(None);
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn validate_dimensions_rejects_out_of_range_values_before_derived_math() {
        assert!(validate_dimensions(4097, 128).is_err());
        assert!(validate_dimensions(128, 31).is_err());
        assert!(validate_dimensions(128, 128).is_ok());
    }
}
