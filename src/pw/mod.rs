use std::{cell::Cell, error::Error, io::ErrorKind, rc::Rc};

mod stream;

use async_std::task;
use futures::SinkExt;
use log::{debug, error, warn};
use pipewire::{
    self as pw, channel,
    context::Context,
    core::Core,
    main_loop::MainLoop,
    spa::{
        param::audio::{AudioFormat, AudioInfoRaw},
        pod::{serialize::PodSerializer, Object, Value},
        utils::Direction,
    },
    stream::{Stream, StreamFlags, StreamListener, StreamRef},
    sys::pw_stream_update_properties,
};
use pw::{properties::properties, spa};
use spa::{pod::Pod, sys};

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
    stream: Option<Rc<PlayerStream>>,
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
            stream: None,
            loop_rx: Some(loop_rx),
            command_tx,
        };

        Ok(client)
    }

    pub fn attach_stream(&mut self, stream: PlayerStream) -> Result<(), Box<dyn Error>> {
        let mut params = AudioParams::new(stream.rate, stream.channels);
        stream.connect(&mut params)?;
        self.stream = Some(Rc::new(stream));

        Ok(())
    }

    pub fn play_song(&mut self) {
        let stream = self.stream.as_ref().unwrap();
        // Bind the receiver here to avoid weird lifetime stuff. The next song needs a
        // new receiver and the command thread needs its sender after we are done playing.
        let _receiver = self.loop_rx.take().unwrap().attach(self.mainloop.loop_(), {
            let stream = stream.clone();
            let mainloop = self.mainloop.clone();
            move |c| match c {
                Command::Volume(vol) => {
                    // Cube volume because https://bugzilla.redhat.com/show_bug.cgi?id=502057
                    let vol = vol * vol * vol;
                    stream.set_volume(vol);
                }
                Command::Skip => {
                    mainloop.quit();
                }
                Command::Play => stream.set_active(true),
                Command::Pause => stream.set_active(false),
                Command::Toggle => stream.toggle_active(),
                _ => {}
            }
        });

        stream.set_name("epic stream part 2");
        self.mainloop.run();

        // Update the command thread with the new tx so it can actually send us commands next song
        let (tx, rx) = channel::channel();
        self.loop_rx = Some(rx);
        let _ = task::block_on(self.command_tx.send(Command::UpdatePwSender(tx)));
    }
}

pub struct PlayerStream {
    stream: Stream,
    // Needs to be kept alive to keep the listener registered
    _listener: StreamListener<()>,
    rate: u32,
    channels: u32,
    active: Cell<bool>,
}

impl PlayerStream {
    pub fn new(mut song: SongReader, client: &PipewireClient) -> Result<Self, Box<dyn Error>> {
        let mainloop = client.mainloop.clone();
        let rate = song.rate;
        let channels = song.channels;

        let stream = create_playback_stream(&client.core, song.channels)?;
        let _listener = stream
            .add_local_listener()
            .process(move |stream, _| Self::on_process(stream, &mut song, &mainloop))
            .register()?;

        Ok(Self {
            stream,
            _listener,
            rate,
            channels,
            active: true.into(),
        })
    }

    pub fn set_name(&self, name: &str) {
        let props = properties! {
            *pw::keys::MEDIA_NAME => name,
        };
        unsafe {
            pw_stream_update_properties(self.stream.as_raw_ptr(), props.dict().as_raw_ptr());
        }
    }

    pub fn connect(&self, params: &mut AudioParams) -> Result<(), Box<dyn Error>> {
        let mut params = [Pod::from_bytes(&params.bytes).unwrap()];
        let flags = StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS;
        self.stream
            .connect(Direction::Output, None, flags, &mut params)?;
        Ok(())
    }

    // See https://bootlin.com/blog/a-custom-pipewire-node/
    pub fn set_volume(&self, volume: f32) {
        let _ = self
            .stream
            .set_control(sys::SPA_PROP_channelVolumes, &[volume, volume]);
    }

    pub fn set_active(&self, state: bool) {
        self.active.set(state);
        let _ = self.stream.set_active(state);
    }

    pub fn toggle_active(&self) {
        let state = self.active.get();
        self.set_active(!state);
    }

    fn on_process(stream: &StreamRef, song: &mut SongReader, mainloop: &MainLoop) {
        // TODO: Re-implement seeking
        // let mut state_lock = state.lock().unwrap();
        // if let Some(time) = state_lock.get_seek() {
        //     let _ = song.seek_time(time);
        // }
        // drop(state_lock);

        match stream.dequeue_buffer() {
            None => println!("no buffer!"),
            Some(mut buffer) => {
                let datas = buffer.datas_mut();
                let num_channels = song.channels as usize;
                let sample_size = std::mem::size_of::<f32>();
                let stride = sample_size * num_channels;

                let data = &mut datas[0];
                let n_frames = if let Some(slice) = data.data() {
                    let output_frame_count = slice.len() / stride;
                    let chunk = match song.next_chunk() {
                        Ok(chunk) => chunk,
                        Err(SongReaderError::DecodeError(e)) => {
                            warn!("Decoding error (not fatal): {e:?}");
                            return;
                        }
                        Err(SongReaderError::IoError(e))
                            if e.kind() == ErrorKind::UnexpectedEof =>
                        {
                            debug!("Song finished");
                            mainloop.quit();
                            return;
                        }
                        Err(e) => {
                            error!("Fatal error playing song: {e:?}");
                            mainloop.quit();
                            return;
                        }
                    };

                    let frames_available = chunk.len() / num_channels;
                    let frames_to_write = output_frame_count.min(frames_available);
                    let total_bytes = frames_to_write * stride;
                    let part = &mut slice[..total_bytes];
                    part.copy_from_slice(&chunk.as_bytes()[..total_bytes]);

                    frames_to_write
                } else {
                    0
                };

                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = stride as _;
                *chunk.size_mut() = (stride * n_frames) as _;
            }
        }
    }
}

fn create_playback_stream(core: &Core, channels: u32) -> Result<Stream, Box<dyn Error>> {
    Ok(pw::stream::Stream::new(
        core,
        "Music playback",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::AUDIO_CHANNELS => format!("{channels}"),
        },
    )?)
}

pub struct AudioParams {
    bytes: Vec<u8>,
}

impl AudioParams {
    pub fn new(rate: u32, channels: u32) -> Self {
        assert_eq!(channels, 2, "Only 2 channel tracks are supported");
        let mut info = AudioInfoRaw::new();
        info.set_format(AudioFormat::F32LE);
        info.set_rate(rate);
        info.set_channels(channels);

        let mut position = [0; spa::param::audio::MAX_CHANNELS];
        position[0] = sys::SPA_AUDIO_CHANNEL_FL;
        position[1] = sys::SPA_AUDIO_CHANNEL_FR;
        info.set_position(position);

        let values: Vec<u8> = PodSerializer::serialize(
            std::io::Cursor::new(vec![]),
            &Value::Object(Object {
                type_: sys::SPA_TYPE_OBJECT_Format,
                id: sys::SPA_PARAM_EnumFormat,
                properties: info.into(),
            }),
        )
        // TODO: don't use unwrap
        .unwrap()
        .0
        .into_inner();

        Self { bytes: values }
    }
}
