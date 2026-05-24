use std::fs;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use worldgen::{RenderConfig, WorldConfig, build_metadata, generate_world, render_world};

#[derive(Debug, Parser)]
#[command(name = "mapgen")]
#[command(about = "Generate seeded tile-based world maps")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Generate {
        #[arg(long)]
        seed: u64,
        #[arg(long, default_value_t = 384)]
        width: usize,
        #[arg(long, default_value_t = 384)]
        height: usize,
        #[arg(long, default_value_t = 4)]
        scale: u32,
        #[arg(long, default_value_t = 0.52)]
        sea_level: f32,
        #[arg(long, default_value_t = 0.0)]
        temperature_bias: f32,
        #[arg(long, default_value_t = 0.0)]
        moisture_bias: f32,
        #[arg(long)]
        out: PathBuf,
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
        Commands::Generate {
            seed,
            width,
            height,
            scale,
            sea_level,
            temperature_bias,
            moisture_bias,
            out,
        } => {
            let config = WorldConfig {
                seed,
                width,
                height,
                sea_level,
                temperature_bias,
                moisture_bias,
                render_scale: scale,
            };
            config.validate()?;

            let world = generate_world(&config)?;
            let image = render_world(&world, RenderConfig { scale });
            let metadata = build_metadata(&world, &config);
            let png_path = out.with_extension("png");
            let json_path = out.with_extension("json");

            if let Some(parent) = png_path.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent).map_err(|err| format!("failed to create output directory: {err}"))?;
            }

            image.save(&png_path).map_err(|err| format!("failed to write PNG: {err}"))?;
            let json = serde_json::to_string_pretty(&metadata)
                .map_err(|err| format!("failed to serialize metadata: {err}"))?;
            fs::write(&json_path, json).map_err(|err| format!("failed to write metadata: {err}"))?;

            println!("wrote {}", png_path.display());
            println!("wrote {}", json_path.display());
            Ok(())
        }
    }
}
