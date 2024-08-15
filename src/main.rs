use std::sync::{Arc, Mutex};

use log::{info, warn};
use pw::{PipewireClient, PlayerStream};
use song::SongReader;
use state::PlayerState;

mod command;
mod pw;
mod song;
mod state;
mod uds;

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logger();
    pipewire::init();
    let files = std::env::args().skip(1);

    // TODO: clean up pipewire stuff
    let state = Arc::new(Mutex::new(PlayerState::new()));
    let mut client = PipewireClient::create(state.clone())?;
    command::start_command_thread(state.clone());

    for file in files {
        let song = match SongReader::from_file(&file) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to load {file}: {e:?}");
                continue;
            }
        };

        // TODO: Support tracks with other channel counts
        if song.channels != 2 {
            warn!("Only 2 channel tracks are supported");
            continue;
        }

        info!(
            "Loaded {file} | {} channels, {} Hz",
            song.channels, song.rate
        );

        let stream = PlayerStream::new(song, &client)?;
        client.attach_stream(stream)?;
        client.play_song();
    }

    Ok(())
}
