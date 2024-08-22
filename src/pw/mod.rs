use std::{error::Error, io::ErrorKind, rc::Rc};

mod audio_info;
mod stream;

use async_std::task;
use futures::SinkExt;
use log::{debug, error, warn};
use pipewire::{channel, context::Context, core::Core, main_loop::MainLoop};
use stream::{Stream, StreamMetadata};

use crate::{
    command::{Command, Sender},
    song::{SongReader, SongReaderError},
};

pub type PipewireLoopTx = channel::Sender<Command>;

// TODO: Handle this better
pub struct PipewireClient {
    mainloop: MainLoop,
    _context: Context,
    loop_rx: Option<channel::Receiver<Command>>,
    command_tx: Sender<Command>,
    core: Core,
}

impl PipewireClient {
    pub fn create(mut command_tx: Sender<Command>) -> Result<Self, Box<dyn Error>> {
        let mainloop = MainLoop::new(None)?;
        let context = Context::new(&mainloop)?;
        let core = context.connect(None)?;

        let (loop_tx, loop_rx) = channel::channel();
        task::block_on(command_tx.send(Command::UpdatePwSender(loop_tx)))?;

        let client = Self {
            mainloop,
            _context: context,
            core,
            loop_rx: Some(loop_rx),
            command_tx,
        };

        Ok(client)
    }

    pub fn play_song(&mut self, mut song: SongReader) -> Result<(), pipewire::Error> {
        let mut stream = Stream::new(
            &self.core,
            StreamMetadata {
                rate: song.rate,
                channels: song.channels,
            },
        )?;

        if let Some(ref name) = song.name {
            let _ = stream.set_name(name);
        } else {
            let _ = stream.set_name("pwplayer");
        }

        stream.set_process_callback({
            let mainloop = self.mainloop.clone();
            move |buffer| {
                let chunk = match song.next_chunk() {
                    Ok(chunk) => chunk,
                    Err(SongReaderError::DecodeError(e)) => {
                        warn!("Decoding error (not fatal): {e:?}");
                        return 0;
                    }
                    Err(SongReaderError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => {
                        debug!("Song finished");
                        mainloop.quit();
                        return 0;
                    }
                    Err(e) => {
                        error!("Fatal error playing song: {e:?}");
                        mainloop.quit();
                        return 0;
                    }
                };

                let samples_to_write = buffer.len().min(chunk.len());
                buffer[..samples_to_write].copy_from_slice(&chunk.samples()[..samples_to_write]);
                // Return the number of samples written per channel
                samples_to_write / 2
            }
        })?;

        stream.connect()?;

        let stream = Rc::new(stream);

        // Bind the receiver here to avoid weird lifetime stuff. The next song needs a
        // new receiver and the command thread needs its sender after we are done playing.
        let _receiver = self.loop_rx.take().unwrap().attach(self.mainloop.loop_(), {
            let mainloop = self.mainloop.clone();
            let stream = stream.clone();
            move |c| match c {
                Command::Volume(vol) => {
                    // Cube volume because https://bugzilla.redhat.com/show_bug.cgi?id=502057
                    let vol = vol * vol * vol;
                    let _ = stream.set_volume(vol);
                }
                Command::Skip => {
                    mainloop.quit();
                }
                Command::Play => {
                    let _ = stream.set_active(true);
                }
                Command::Pause => {
                    let _ = stream.set_active(false);
                }
                Command::Toggle => {
                    warn!("Toggle not implemented");
                }
                _ => {}
            }
        });

        self.mainloop.run();

        // Update the command thread with the new tx so it can actually send us commands next song
        let (tx, rx) = channel::channel();
        self.loop_rx = Some(rx);
        let _ = task::block_on(self.command_tx.send(Command::UpdatePwSender(tx)));
        Ok(())
    }
}
