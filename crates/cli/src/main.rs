use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use rand::random;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::FormatItem;
use time::macros::format_description;
use worldgen::{RenderConfig, World, WorldConfig, build_metadata, generate_world, render_world};

const TILES_SCHEMA_VERSION: u32 = 11;

#[derive(Serialize, Deserialize)]
struct TileExport {
    #[serde(default)]
    schema_version: u32,
    #[serde(flatten)]
    world: World,
}

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
    Render(RenderArgs),
    Generate(GenerateArgs),
}

#[derive(Debug, Args)]
struct RenderArgs {
    /// Path to a tiles.json file (or a run directory containing one).
    #[arg(long)]
    input: PathBuf,
}

#[derive(Debug, Args)]
struct GenerateArgs {
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
    /// Tiles per world unit. 0 or omit = match min(width, height) for a single world unit.
    /// Set to a fixed value (e.g. 384) to make larger maps cover more geographic area.
    #[arg(long, default_value_t = 0)]
    world_size: u32,
    #[arg(long, default_value = "output")]
    out_dir: PathBuf,
    /// Export full per-tile data as tiles.json alongside the PNG.
    #[arg(long, default_value_t = false)]
    export_tiles: bool,
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
        Commands::Render(args) => run_render(args),
        Commands::Generate(args) => run_generate(args),
    }
}

fn run_render(args: RenderArgs) -> Result<(), String> {
    let tiles_path = tiles_path_from_input(&args.input);
    let run_dir = tiles_path
        .parent()
        .ok_or("tiles.json has no parent directory")?
        .to_path_buf();
    let json = fs::read_to_string(&tiles_path)
        .map_err(|err| format!("failed to read {}: {err}", tiles_path.display()))?;
    let export: TileExport =
        serde_json::from_str(&json).map_err(|err| format!("failed to parse tiles.json: {err}"))?;
    if export.schema_version != TILES_SCHEMA_VERSION {
        eprintln!(
            "warning: tiles.json schema version {} (current: {}); some fields may be missing or ignored",
            export.schema_version, TILES_SCHEMA_VERSION
        );
    }

    let world = export.world;
    validate_render_world(&world)?;
    let image = render_world(
        &world,
        RenderConfig {
            scale: render_scale_for_dimensions(world.width, world.height),
        },
    );
    let out_path = run_dir.join("rerendered.png");
    image
        .save(&out_path)
        .map_err(|err| format!("failed to write PNG: {err}"))?;
    println!("wrote {}", out_path.display());
    Ok(())
}

fn run_generate(args: GenerateArgs) -> Result<(), String> {
    let seed = select_seed(args.seed);
    validate_dimensions(args.width, args.height)?;

    // pixels/tile is always derived from the base (unscaled) dimensions so that
    // --scale never changes visual density, only world size.
    let render_scale = render_scale_for_dimensions(args.width, args.height);
    let (width, height, world_size) =
        scaled_dimensions(args.width, args.height, args.scale, args.world_size);
    let config = WorldConfig {
        seed,
        width,
        height,
        sea_level: args.sea_level,
        temperature_bias: args.temperature_bias,
        moisture_bias: args.moisture_bias,
        rainfall_scale: args.rainfall_scale,
        render_scale,
        world_size,
    };
    config.validate()?;

    let world = generate_world(&config)?;
    write_generation_outputs(&args.out_dir, seed, world, &config, args.export_tiles)
}

fn tiles_path_from_input(input: &Path) -> PathBuf {
    if input.is_dir() {
        input.join("tiles.json")
    } else {
        input.to_path_buf()
    }
}

fn render_scale_for_dimensions(width: usize, height: usize) -> u32 {
    (1536_u32 / width.max(height) as u32).clamp(1, 32)
}

fn scaled_dimensions(
    width: usize,
    height: usize,
    scale: Option<u32>,
    world_size: u32,
) -> (usize, usize, u32) {
    match scale {
        Some(s) if s > 1 => {
            let scaled_width = width.saturating_mul(s as usize).min(4096);
            let scaled_height = height.saturating_mul(s as usize).min(4096);
            // If world_size was not set explicitly, fix it to the base tile count
            // so each tile covers the same geographic area at any scale factor.
            let scaled_world_size = if world_size == 0 {
                width.min(height) as u32
            } else {
                world_size
            };
            (scaled_width, scaled_height, scaled_world_size)
        }
        _ => (width, height, world_size),
    }
}

fn write_generation_outputs(
    out_dir: &Path,
    seed: u64,
    world: World,
    config: &WorldConfig,
    export_tiles: bool,
) -> Result<(), String> {
    let image = render_world(
        &world,
        RenderConfig {
            scale: config.render_scale,
        },
    );
    let metadata = build_metadata(&world, config);
    let run_dir = build_run_output_dir(out_dir, seed, OffsetDateTime::now_utc())?;
    let png_path = run_dir.join("map.png");
    let json_path = run_dir.join("metadata.json");

    fs::create_dir_all(&run_dir)
        .map_err(|err| format!("failed to create output directory: {err}"))?;

    image
        .save(&png_path)
        .map_err(|err| format!("failed to write PNG: {err}"))?;
    let json = serde_json::to_string_pretty(&metadata)
        .map_err(|err| format!("failed to serialize metadata: {err}"))?;
    fs::write(&json_path, json).map_err(|err| format!("failed to write metadata: {err}"))?;
    if export_tiles {
        let tiles_path = run_dir.join("tiles.json");
        let export = TileExport {
            schema_version: TILES_SCHEMA_VERSION,
            world,
        };
        let tiles_json = serde_json::to_string(&export)
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

fn validate_render_world(world: &World) -> Result<(), String> {
    if world.width == 0 || world.height == 0 {
        return Err("tiles.json width and height must be greater than 0".into());
    }
    if world.width > 4096 || world.height > 4096 {
        return Err("tiles.json width and height must be at most 4096".into());
    }
    let expected = world
        .width
        .checked_mul(world.height)
        .ok_or("tiles.json dimensions overflow tile count")?;
    if world.tiles.len() != expected {
        return Err(format!(
            "tiles.json tile count mismatch: expected {} for {}x{}, found {}",
            expected,
            world.width,
            world.height,
            world.tiles.len()
        ));
    }
    Ok(())
}

fn build_run_output_dir(base: &Path, seed: u64, now: OffsetDateTime) -> Result<PathBuf, String> {
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

    #[test]
    fn render_scale_tracks_base_dimensions() {
        assert_eq!(render_scale_for_dimensions(384, 384), 4);
        assert_eq!(render_scale_for_dimensions(4096, 128), 1);
        assert_eq!(render_scale_for_dimensions(32, 32), 32);
    }

    #[test]
    fn scaled_dimensions_preserve_geographic_scale_defaults() {
        assert_eq!(scaled_dimensions(384, 192, None, 0), (384, 192, 0));
        assert_eq!(scaled_dimensions(384, 192, Some(2), 0), (768, 384, 192));
        assert_eq!(scaled_dimensions(384, 192, Some(2), 512), (768, 384, 512));
    }

    #[test]
    fn legacy_tile_export_without_schema_version_deserializes_as_version_zero() {
        let world = World::new(7, 2, 2, 0.52, 0);
        let json = serde_json::to_string(&world).unwrap();
        let export: TileExport = serde_json::from_str(&json).unwrap();

        assert_eq!(export.schema_version, 0);
        assert_eq!(export.world.seed, 7);
        assert_eq!(export.world.tiles.len(), 4);
    }

    #[test]
    fn current_tile_export_schema_is_version_eleven() {
        assert_eq!(TILES_SCHEMA_VERSION, 11);
    }

    #[test]
    fn validate_render_world_rejects_tile_count_mismatch() {
        let mut world = World::new(7, 2, 2, 0.52, 0);
        world.tiles.pop();

        let err = validate_render_world(&world).unwrap_err();
        assert!(err.contains("tile count mismatch"));
    }
}
