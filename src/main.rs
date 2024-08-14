use std::{
    fs::File,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex},
};

use symphonia::{
    core::{
        audio::RawSampleBuffer, codecs::Decoder, formats::FormatReader, io::MediaSourceStream,
        probe::Hint,
    },
    default,
};

use pipewire::{
    self as pw,
    spa::pod::{serialize::PodSerializer, Object, Value},
    stream::StreamFlags,
};
use pw::{properties::properties, spa};
use spa::{pod::Pod, sys};

fn get_stream_from_file<T: AsRef<Path>>(file: T) -> (Box<dyn FormatReader>, Box<dyn Decoder>) {
    let codecs = default::get_codecs();
    let probe = default::get_probe();

    let stream = MediaSourceStream::new(Box::new(File::open(file).unwrap()), Default::default());
    let reader = probe
        .format(
            &Hint::default(),
            stream,
            &Default::default(),
            &Default::default(),
        )
        .expect("failed to probe stream")
        .format;
    let track = reader.default_track().expect("no tracks in file");
    let decoder = codecs
        .make(&track.codec_params, &Default::default())
        .expect("failed to make decoder");
    (reader, decoder)
}

struct PlayerState {
    paused: bool,
    volume: f32,
}

impl PlayerState {
    pub fn new() -> Self {
        Self {
            paused: false,
            volume: 1.0,
        }
    }

    pub fn play(&mut self) {
        self.paused = false;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn toggle(&mut self) {
        self.paused = !self.paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }

    pub fn get_volume(&self) -> f32 {
        self.volume
    }
}

enum Command {
    Play,
    Pause,
    Toggle,
    Done,
    Volume(f32),
    Quit,
}

impl FromStr for Command {
    type Err = Box<dyn std::error::Error>;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut parts = s.split_whitespace();
        match parts.next().ok_or("Invalid command")? {
            "play" => Ok(Self::Play),
            "pause" => Ok(Self::Pause),
            "toggle" => Ok(Self::Toggle),
            "quit" => Ok(Self::Quit),
            "done" => Ok(Self::Done),
            "volume" => {
                let volume = parts.next().ok_or("Expected argument")?.parse()?;
                Ok(Self::Volume(volume))
            }
            _ => Err("Invalid command".into()),
        }
    }
}

fn read_some_bytes(sock: &mut UnixStream) -> Result<String, Box<dyn std::error::Error>> {
    let mut buf = [0u8; 128];
    let len = sock.read(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf[..len]).to_string())
}

fn handle_sock(
    mut sock: UnixStream,
    player: &Mutex<PlayerState>,
) -> Result<bool, Box<dyn std::error::Error>> {
    loop {
        let msg = read_some_bytes(&mut sock)?;
        let msg = msg.trim();
        let command: Command = match msg.parse() {
            Ok(c) => {
                let _ = sock.write_all(b"ack\n");
                c
            }
            Err(e) => {
                let _ = sock.write_all(format!("bad command: {e:?}\n").as_bytes());
                continue;
            }
        };

        match command {
            Command::Play => player.lock().unwrap().play(),
            Command::Pause => player.lock().unwrap().pause(),
            Command::Toggle => {
                player.lock().unwrap().toggle();
            }
            Command::Volume(vol) => player.lock().unwrap().set_volume(vol),
            Command::Quit => return Ok(true),
            Command::Done => return Ok(false),
        }
    }
}

fn do_threading(state: Arc<Mutex<PlayerState>>) {
    let _ = std::fs::remove_file("/tmp/pwplayer.sock");
    let sock = UnixListener::bind("/tmp/pwplayer.sock").unwrap();

    loop {
        match sock.accept() {
            Ok((sock, _)) => match handle_sock(sock, &state) {
                Ok(true) => {
                    let _ = std::fs::remove_file("/tmp/pwplayer.sock");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("command error: {e:?}")
                }
                _ => {}
            },
            Err(e) => eprintln!("socket error: {e:?}"),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pw::init();
    let file = std::env::args().skip(1).next().expect("invalid args");
    let (mut reader, mut decoder) = get_stream_from_file(&file);

    let mainloop = pw::main_loop::MainLoop::new(None)?;
    let context = pw::context::Context::new(&mainloop)?;
    let core = context.connect(None)?;

    let state = Arc::new(Mutex::new(PlayerState::new()));
    let state_clone = state.clone();
    let loop_clone = mainloop.clone();

    std::thread::spawn(move || do_threading(state_clone));

    let stream = pw::stream::Stream::new(
        &core,
        "epic music",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::AUDIO_CHANNELS => "2",
        },
    )?;

    let mut samples: Option<RawSampleBuffer<f32>> = None;

    let _listener = stream
        .add_local_listener_with_user_data(())
        .process(move |stream, _| {
            let state_lock = state.lock().unwrap();
            if state_lock.is_paused() {
                return;
            }
            let volume = state_lock.get_volume();
            drop(state_lock);
            match stream.dequeue_buffer() {
                None => println!("no buffer!"),
                Some(mut buffer) => {
                    let datas = buffer.datas_mut();
                    let num_channels = 2;
                    let stride = std::mem::size_of::<f32>() * num_channels;
                    let data = &mut datas[0];
                    let n_frames = if let Some(slice) = data.data() {
                        let n_frames = slice.len() / stride;
                        let packet = reader.next_packet();
                        if packet.is_err() {
                            loop_clone.quit();
                            return;
                        }
                        let decoded = decoder
                            .decode(&packet.unwrap())
                            .expect("Failed to decode packet");
                        if samples.is_none() {
                            let _ = samples.replace(RawSampleBuffer::new(
                                decoded.capacity() as u64,
                                *decoded.spec(),
                            ));
                        }
                        let samples = samples.as_mut().unwrap();
                        samples.copy_interleaved_ref(decoded);
                        let sample_bytes = samples.as_bytes();
                        let actual_frames = n_frames.min(samples.len() / 2);
                        for i in 0..actual_frames {
                            for c in 0..num_channels {
                                let start = i * stride + (c as usize * std::mem::size_of::<f32>());
                                let end = start + std::mem::size_of::<f32>();
                                let val = f32::from_le_bytes(
                                    sample_bytes[start..end].try_into().unwrap(),
                                ) * volume;
                                let chan = &mut slice[start..end];
                                chan.copy_from_slice(&val.to_le_bytes());
                            }
                        }
                        actual_frames
                    } else {
                        0
                    };

                    let chunk = data.chunk_mut();
                    *chunk.offset_mut() = 0;
                    *chunk.stride_mut() = stride as _;
                    *chunk.size_mut() = (stride * n_frames) as _;
                }
            }
        })
        .register()?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(48000);
    audio_info.set_channels(2);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = sys::SPA_AUDIO_CHANNEL_FR;
    audio_info.set_position(position);

    let values: Vec<u8> = PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &Value::Object(Object {
            type_: sys::SPA_TYPE_OBJECT_Format,
            id: sys::SPA_PARAM_EnumFormat,
            properties: audio_info.into(),
        }),
    )
    .unwrap()
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).unwrap()];

    stream.connect(
        spa::utils::Direction::Output,
        None,
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    mainloop.run();

    Ok(())
}
