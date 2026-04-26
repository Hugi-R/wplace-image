use std::fs;
use std::io::Cursor;
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use chrono::{DateTime, Utc};
use wimage::{PalettedImage};
use wimage::imageprocessing::{downscale_mode_weighted};
use wimage::tilehistory::{TileHistory, DateHours};

#[derive(Parser)]
#[command(name = "wimage_cli")]
#[command(about = "CLI for wimage library")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Convert {
        input: String,
        output: String,
    },
    Decompress {
        input: String,
        output: String,
        #[arg(short, long, default_value_t = false)]
        keep_diff: bool,
    },
    Downscale {
        input: String,
        output: String,
        #[arg(short, long, default_value = "2")]
        factor: u32,
        #[arg(short, long, default_value = "uniform")]
        weights: String,
    },
    Diff {
        base: String,
        changed: String,
        output: String,
    },
    History {
        #[command(subcommand)]
        action: HistoryCommands,
    },
}

#[derive(Subcommand)]
enum HistoryCommands {
    Create {
        output: String,
    },
    Add {
        input: String,
        #[arg(short, long)]
        date: String,
        #[arg(short, long)]
        image: String,
    },
    Get {
        input: String,
        #[arg(short, long)]
        date: String,
        #[arg(short, long)]
        output: String,
    },
    List {
        input: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Convert { input, output } => {
            let data = fs::read(&input)?;
            let reader = Cursor::new(data);
            let paletted = PalettedImage::from_png(reader)?;
            let compressed = paletted.to_compressed_bytes()?;
            fs::write(&output, &compressed.0)?;
        }
        Commands::Decompress { input, output , keep_diff} => {
            let compressed = fs::read(&input)?;
            let paletted = PalettedImage::from_compressed_bytes(&compressed)?;
            let png_bytes = if keep_diff {
                paletted.to_png_diff()?
            } else {
                paletted.to_png()?
            };
            fs::write(&output, png_bytes)?;
        }
        Commands::Downscale { input, output, factor, weights } => {
            let compressed = fs::read(&input)?;
            let paletted = PalettedImage::from_compressed_bytes(&compressed)?;
            let weight_array = parse_weights(&weights)?;
            let new_paletted = downscale_mode_weighted(&paletted, &weight_array, factor as usize);
            let new_compressed = new_paletted.to_compressed_bytes()?;
            fs::write(&output, &new_compressed.0)?;
        }
        Commands::Diff { base, changed, output } => {
            let base_compressed = fs::read(&base)?;
            let changed_compressed = fs::read(&changed)?;
            let base_paletted = PalettedImage::from_compressed_bytes(&base_compressed)?;
            let changed_paletted = PalettedImage::from_compressed_bytes(&changed_compressed)?;
            let (_any_diff, diff_paletted) = base_paletted.diff(&changed_paletted);
            let diff_compressed = diff_paletted.to_compressed_bytes()?;
            fs::write(&output, &diff_compressed.0)?;
        }
        Commands::History { action } => match action {
            HistoryCommands::Create { output } => {
                let history = TileHistory { imgs: HashMap::new() };
                let serialized = history.to_bytes();
                fs::write(&output, serialized)?;
            }
            HistoryCommands::Add { input, date, image } => {
                let history_bytes = fs::read(&input)?;
                let mut history = TileHistory::from_bytes(&history_bytes)?;
                let date_hours = parse_date(&date)?;
                let image_compressed = fs::read(&image)?;
                let paletted = PalettedImage::from_compressed_bytes(&image_compressed)?;
                history.add(date_hours, paletted)?;
                let new_serialized = history.to_bytes();
                fs::write(&input, new_serialized)?;
            }
            HistoryCommands::Get { input, date, output } => {
                let history_bytes = fs::read(&input)?;
                let history = TileHistory::from_bytes(&history_bytes)?;
                let date_hours = parse_date(&date)?;
                let paletted = history.get(date_hours)?;
                let compressed = paletted.to_compressed_bytes()?;
                fs::write(&output, &compressed.0)?;
            }
            HistoryCommands::List { input } => {
                let history_bytes = fs::read(&input)?;
                let history = TileHistory::from_bytes(&history_bytes)?;
                let dates = history.list();
                for date in dates {
                    println!("{} - {}", date.0, date.to_datetime());
                }
            }
        },
    }

    Ok(())
}

fn parse_weights(weights: &str) -> Result<[u32; 256]> {
    match weights {
        "uniform" => Ok([100; 256]),
        "transparent-zero" => {
            let mut arr = [100; 256];
            arr[0] = 0; // transparent
            Ok(arr)
        }
        file_path => {
            let content = fs::read_to_string(file_path)?;
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() != 256 {
                return Err(anyhow!("Weights file must have exactly 256 lines"));
            }
            let mut arr = [0; 256];
            for (i, line) in lines.iter().enumerate() {
                arr[i] = line.trim().parse()?;
            }
            Ok(arr)
        }
    }
}

fn parse_date(date_str: &str) -> Result<DateHours> {
    let dt = DateTime::parse_from_rfc3339(date_str)?.with_timezone(&Utc);
    let hours = DateHours::from_datetime(dt);
    Ok(hours)
}