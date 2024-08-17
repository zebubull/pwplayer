use std::str::FromStr;

use async_std::{
    io::{prelude::BufReadExt, BufReader},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    stream::StreamExt,
    task,
};
use futures::{channel::mpsc, SinkExt};
use log::{debug, warn};
use symphonia::core::units::Time;

use crate::pw::PipewireLoopTx;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub type Sender<T> = mpsc::UnboundedSender<T>;
pub type Receiver<T> = mpsc::UnboundedReceiver<T>;

#[derive(Clone)]
pub enum Command {
    // For pipewire thread
    Play,
    Pause,
    Toggle,
    Volume(f32),
    Seek(Time),
    Skip,
    // For this thread
    UpdatePwSender(PipewireLoopTx),
    // For application
    Done,
    Quit,
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::Play => write!(f, "Command::Play"),
            Command::Pause => write!(f, "Command::Pause"),
            Command::Toggle => write!(f, "Command::Toggle"),
            Command::Volume(v) => write!(f, "Command::Volume({v})"),
            Command::Seek(s) => write!(f, "Command::Seek({s:?})"),
            Command::Skip => write!(f, "Command::Skip"),
            Command::UpdatePwSender(_) => write!(f, "Command::UpdatePwSender(_)"),
            Command::Done => write!(f, "Command::Done"),
            Command::Quit => write!(f, "Command::Quit"),
        }
    }
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

pub fn start_command_thread() -> Sender<Command> {
    let (tx, rx) = mpsc::unbounded();
    let tx_clone = tx.clone();
    std::thread::spawn(move || task::block_on(accept_clients("/tmp/pwplayer.sock", tx_clone)));
    std::thread::spawn(move || task::block_on(handle_messages(rx)));
    tx
}

async fn handle_messages(mut message_rx: Receiver<Command>) -> Result<()> {
    let mut channel = None;
    while let Some(msg) = message_rx.next().await {
        match msg {
            Command::UpdatePwSender(s) => channel = Some(s),
            _ => {
                let _ = channel.as_ref().map(|c| c.send(msg));
            }
        }
    }
    Ok(())
}

async fn accept_clients(path: impl AsRef<Path>, message_tx: Sender<Command>) -> Result<()> {
    let _ = async_std::fs::remove_file(&path).await;
    let listener = UnixListener::bind(&path).await?;
    let mut incoming = listener.incoming();

    while let Some(stream) = incoming.next().await {
        let stream = stream?;
        debug!("New client connected");
        let tx_clone = message_tx.clone();
        task::spawn(async move {
            if let Err(e) = handle_client(stream, tx_clone).await {
                warn!("Client error: {e:?}");
            }
        });
    }

    Ok(())
}

async fn handle_client(stream: UnixStream, mut message_tx: Sender<Command>) -> Result<()> {
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
            _ => message_tx.send(c).await.unwrap(),
        }
    }
    Ok(())
}
