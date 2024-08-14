use std::sync::{Arc, Mutex};

use song::SongReader;
use state::PlayerState;

use pipewire::{
    self as pw,
    spa::pod::{serialize::PodSerializer, Object, Value},
    stream::StreamFlags,
};
use pw::{properties::properties, spa};
use spa::{pod::Pod, sys};

mod command;
mod song;
mod state;
mod uds;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pw::init();
    let file = std::env::args().skip(1).next().expect("invalid args");
    let mut song = SongReader::from_file(&file)?;
    let rate = song.rate;
    let channels = song.channels;

    println!("Loaded {file}\n {channels} channels\n {rate} Hz");

    // TODO: Support tracks with other channel counts
    assert_eq!(channels, 2, "Only 2 channel tracks are supported");

    // TODO: clean up pipewire stuff
    let mainloop = pw::main_loop::MainLoop::new(None)?;
    let context = pw::context::Context::new(&mainloop)?;
    let core = context.connect(None)?;

    let state = Arc::new(Mutex::new(PlayerState::new()));
    let loop_clone = mainloop.clone();

    command::start_command_thread(state.clone());

    let stream = pw::stream::Stream::new(
        &core,
        "epic music",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::AUDIO_CHANNELS => format!("{channels}"),
        },
    )?;

    let _listener = stream
        .add_local_listener_with_user_data(())
        .process(move |stream, _| {
            let mut state_lock = state.lock().unwrap();
            if state_lock.is_paused() {
                return;
            }
            // TODO: actually use this
            let _volume = state_lock.get_volume();
            if let Some(time) = state_lock.get_seek() {
                let _ = song.seek_time(time);
            }
            drop(state_lock);

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
                            Err(e) => {
                                eprintln!("Decoding error: {e:?}");
                                return loop_clone.quit();
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
        })
        .register()?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(rate);
    audio_info.set_channels(channels);
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
