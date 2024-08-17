use std::{
    error::Error,
    str::FromStr,
    sync::{Arc, RwLock},
};

use log::{debug, error, info, warn};
use symphonia::core::units::Time;

use crate::{
    state::PlayerState,
    uds::{UnixClient, UnixSocket},
};

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

#[derive(Debug, Clone, Copy)]
enum ControlFlow {
    Continue,
    Quit,
}

fn handle_client(
    mut client: UnixClient,
    state: Arc<RwLock<PlayerState>>,
) -> Result<ControlFlow, Box<dyn Error>> {
    loop {
        let data = client.read_line()?;
        let command: Command = match data.parse() {
            Ok(c) => {
                debug!("Received command from client: {c:?}");
                c
            }
            Err(e) => {
                let e = format!("{e:?}");
                warn!("Invalid command from client: {e}");
                client.send_message(&e)?;
                continue;
            }
        };

        match command {
            Command::Quit => return Ok(ControlFlow::Quit),
            Command::Done => return Ok(ControlFlow::Continue),
            Command::Seek(t) => state.write().unwrap().seek_to(t),
            Command::Volume(_)
            | Command::Skip
            | Command::Play
            | Command::Pause
            | Command::Toggle => state.read().unwrap().send(command),
        }
    }
}

fn do_command_thread(state: Arc<RwLock<PlayerState>>) -> Result<(), Box<dyn Error>> {
    let sock = UnixSocket::create("/tmp/pwplayer.sock")?;
    loop {
        match sock.accept() {
            Ok(client) => {
                info!("Client connected");
                match handle_client(client, state.clone()) {
                    // TODO: more elegant quit-out
                    Ok(ControlFlow::Quit) => std::process::exit(0),
                    Err(e) => warn!("Client error: {e:?}"),
                    _ => {}
                }
                info!("Client disconnected");
            }
            Err(e) => warn!("Failed to accept client: {e:?}"),
        }
    }
}

pub fn start_command_thread(state: Arc<RwLock<PlayerState>>) {
    std::thread::spawn(move || match do_command_thread(state) {
        Ok(()) => {}
        Err(e) => error!("Fatal error on command thread: {e:?}"),
    });
}
