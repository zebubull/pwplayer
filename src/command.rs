use std::{
    str::FromStr,
    sync::{Arc, RwLock},
};

use async_std::{
    io::{prelude::BufReadExt, BufReader},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    stream::StreamExt,
    task,
};
use log::{debug, warn};
use symphonia::core::units::Time;

use crate::state::PlayerState;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Play,
    Pause,
    Toggle,
    Done,
    Volume(f32),
    Seek(Time),
    Quit,
    Skip,
}

impl FromStr for Command {
    type Err = Box<dyn std::error::Error>;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut parts = s.split_whitespace();

        match parts.next().ok_or("Empty command")? {
            "play" => Ok(Self::Play),
            "pause" => Ok(Self::Pause),
            "toggle" => Ok(Self::Toggle),
            "quit" => Ok(Self::Quit),
            "done" => Ok(Self::Done),
            "skip" => Ok(Self::Skip),
            "volume" | "vol" => {
                let volume: f32 = parts.next().ok_or("Expected argument")?.parse()?;
                Ok(Self::Volume(volume / 100f32))
            }
            "seek" => {
                // TODO: parse this better
                let seek = parts.next().ok_or("Expected argument")?;
                let time = Time::from_ss(seek.parse()?, 0).ok_or("Failed to convert to time")?;
                Ok(Self::Seek(time))
            }
            _ => Err("Unrecognized command".into()),
        }
    }
}

pub fn start_command_thread(state: Arc<RwLock<PlayerState>>) {
    std::thread::spawn(move || task::block_on(accept_clients("/tmp/pwplayer.sock", state)));
}

async fn accept_clients(path: impl AsRef<Path>, state: Arc<RwLock<PlayerState>>) -> Result<()> {
    let _ = async_std::fs::remove_file(&path).await;
    let listener = UnixListener::bind(&path).await?;
    let mut incoming = listener.incoming();

    while let Some(stream) = incoming.next().await {
        let stream = stream?;
        debug!("New client connected");
        let state = state.clone();
        task::spawn(async move {
            if let Err(e) = handle_client(stream, state).await {
                warn!("Client error: {e:?}");
            }
        });
    }

    Ok(())
}

async fn handle_client(stream: UnixStream, state: Arc<RwLock<PlayerState>>) -> Result<()> {
    let reader = BufReader::new(&stream);
    let mut lines = reader.lines();

    while let Some(line) = lines.next().await {
        let line = line?;
        let c: Command = match line.parse() {
            Ok(c) => {
                debug!("Recieved command: {c:?}");
                c
            }
            Err(e) => {
                warn!("Bad command from client: {e}");
                continue;
            }
        };

        match c {
            Command::Done => return Ok(()),
            Command::Quit => std::process::exit(0),
            Command::Seek(_c) => warn!("Seek not implemented"),
            _ => state.read().unwrap().send(c),
        }
    }
    Ok(())
}
