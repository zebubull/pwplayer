use std::{
    error::Error,
    path::{Path, PathBuf},
};

use log::{info, warn};
use pw::{PipewireClient, PlayerStream};
use rand::seq::SliceRandom;
use song::SongReader;

mod command;
mod pw;
mod song;

fn init_logger() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var(
            "RUST_LOG",
            if cfg!(debug_assertions) {
                "trace"
            } else {
                "warn"
            },
        )
    }

    pretty_env_logger::init();
}

fn walk_dir_recursive<T: AsRef<Path>>(
    dir: T,
    data: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let dir = std::fs::read_dir(dir)?;
    for file in dir.flatten() {
        let path = file.path();
        if path.is_dir() {
            walk_dir_recursive(path, data)?;
        } else if path.is_file() {
            data.push(path);
        }
    }
    Ok(())
}

fn walk_dir<T: AsRef<Path>>(dir: T) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = vec![];
    walk_dir_recursive(dir, &mut files)?;
    Ok(files)
}

fn handle_input_path<T: AsRef<Path>>(path: T) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let md = std::fs::metadata(&path)?;
    if md.is_file() {
        Ok(vec![path.as_ref().to_owned()])
    } else {
        walk_dir(path)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logger();
    pipewire::init();
    // TODO: better cli
    let dir = std::env::args().skip(1).next().expect("Invalid args");
    let mut files = handle_input_path(dir)?;
    // Shuffle for fun
    files.shuffle(&mut rand::thread_rng());

    let tx = command::start_command_thread();
    let mut client = PipewireClient::create(tx)?;

    for file in files {
        let file_pretty = file.display().to_string();
        let song = match SongReader::from_file(&file) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to load {file_pretty}: {e:?}");
                continue;
            }
        };

        // TODO: Support tracks with other channel counts
        if song.channels != 2 {
            warn!("Only 2 channel tracks are supported");
            continue;
        }

        info!(
            "Loaded {} | {} channels, {} Hz",
            if song.name.is_some() {
                song.name.as_ref().unwrap()
            } else {
                &file_pretty
            },
            song.channels,
            song.rate
        );

        let stream = PlayerStream::new(song, &client)?;
        client.attach_stream(stream)?;
        client.play_song();
    }

    Ok(())
}
