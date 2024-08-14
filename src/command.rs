use std::{
    error::Error,
    str::FromStr,
    sync::{Arc, Mutex},
};

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
            "volume" | "vol" => {
                let volume = parts.next().ok_or("Expected argument")?.parse()?;
                Ok(Self::Volume(volume))
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
    state: Arc<Mutex<PlayerState>>,
) -> Result<ControlFlow, Box<dyn Error>> {
    loop {
        let data = client.read_string()?;
        let command: Command = match data.parse() {
            Ok(c) => c,
            Err(e) => {
                let e = format!("{e:?}");
                eprintln!("Invalid command: {e}");
                client.send_message(&e)?;
                continue;
            }
        };

        match command {
            Command::Play => state.lock().unwrap().play(),
            Command::Pause => state.lock().unwrap().pause(),
            Command::Toggle => {
                state.lock().unwrap().toggle();
            }
            Command::Volume(vol) => state.lock().unwrap().set_volume(vol),
            Command::Quit => return Ok(ControlFlow::Quit),
            Command::Done => return Ok(ControlFlow::Continue),
            Command::Seek(t) => state.lock().unwrap().seek_to(t),
        }
    }
}

fn do_command_thread(state: Arc<Mutex<PlayerState>>) -> Result<(), Box<dyn Error>> {
    let sock = UnixSocket::create("/tmp/pwplayer.sock")?;
    loop {
        match sock.accept() {
            Ok(client) => match handle_client(client, state.clone()) {
                // TODO: more elegant quit-out
                Ok(ControlFlow::Quit) => std::process::exit(0),
                Err(e) => eprintln!("Client error: {e:?}"),
                _ => {}
            },
            Err(e) => eprintln!("Failed to accept client: {e:?}"),
        }
    }
}

pub fn start_command_thread(state: Arc<Mutex<PlayerState>>) {
    std::thread::spawn(move || match do_command_thread(state) {
        Ok(()) => {}
        Err(e) => eprintln!("Fatal error on command thread: {e:?}"),
    });
}
