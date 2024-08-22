use log::{debug, warn};
use pipewire::{
    core::Core,
    keys,
    properties::properties,
    spa::{
        self,
        param::audio::AudioFormat,
        pod::{serialize::GenError, Pod},
        utils::{result::SpaResult, Direction},
    },
    stream::{Stream as PwStream, StreamFlags, StreamListener, StreamRef},
};

use super::audio_info::AudioInfo;

#[derive(Debug, Clone, Copy)]
pub struct StreamMetadata {
    pub rate: u32,
    pub channels: u32,
}

pub struct Stream {
    _listener: StreamListener<()>,
    metadata: StreamMetadata,
    stream: PwStream,
}

impl Stream {
    pub fn new(core: &Core, metadata: StreamMetadata) -> Result<Stream, pipewire::Error> {
        assert_eq!(metadata.channels, 2, "Only 2 channel tracks are supported");

        let props = properties! {
            *keys::MEDIA_TYPE => "Audio",
            *keys::MEDIA_ROLE => "Music",
            *keys::MEDIA_CATEGORY => "Playback",
            *keys::AUDIO_CHANNELS => "2",
        };

        let stream = PwStream::new(core, "Stream", props)?;
        let listener = stream.add_local_listener().register()?;

        debug!("Created stream: {metadata:?}");

        Ok(Self {
            _listener: listener,
            stream,
            metadata,
        })
    }

    pub fn set_process_callback<F>(&mut self, mut callback: F) -> Result<(), pipewire::Error>
    where
        F: FnMut(&mut [f32]) -> usize + 'static,
    {
        let metadata = self.metadata.clone();
        self._listener = self
            .stream
            .add_local_listener()
            .process(move |stream, _| stream_process_callback(stream, metadata, &mut callback))
            .register()
            .map_err(|e| {
                warn!("Failed to set process callback on stream: {e:?}");
                e
            })?;
        Ok(())
    }

    pub fn set_active(&self, active: bool) -> Result<(), pipewire::Error> {
        self.stream.set_active(active).map_err(|e| {
            warn!("Error setting stream active state: {e:?}");
            e
        })
    }

    pub fn set_volume(&self, volume: f32) -> Result<(), pipewire::Error> {
        self.stream
            .set_control(spa::sys::SPA_PROP_channelVolumes, &[volume, volume])
            .map_err(|e| {
                warn!("Error setting stream volume: {e:?}");
                e
            })
    }

    pub fn set_name<T: AsRef<str>>(&self, name: T) -> Result<(), pipewire::Error> {
        let props = properties! {
            *keys::MEDIA_NAME => name.as_ref()
        };

        let res = unsafe {
            pipewire::sys::pw_stream_update_properties(
                self.stream.as_raw_ptr(),
                props.dict().as_raw_ptr(),
            )
        };

        SpaResult::from_c(res).into_sync_result().map_err(|e| {
            warn!("Error setting stream name: {e:?}");
            e
        })?;
        Ok(())
    }

    pub fn connect(&self) -> Result<(), pipewire::Error> {
        // TODO: No unwrap
        let params = self.params().unwrap();
        let mut params = [Pod::from_bytes(&params).unwrap()];

        self.stream.connect(
            Direction::Output,
            None,
            StreamFlags::AUTOCONNECT | StreamFlags::RT_PROCESS | StreamFlags::MAP_BUFFERS,
            &mut params,
        )
    }

    pub fn params(&self) -> Result<Vec<u8>, GenError> {
        AudioInfo::new(
            self.metadata.rate,
            self.metadata.channels,
            AudioFormat::F32LE,
        )
        .serialize()
    }
}

fn stream_process_callback<F>(stream: &StreamRef, metadata: StreamMetadata, user_callback: &mut F)
where
    F: FnMut(&mut [f32]) -> usize + 'static,
{
    let mut buffer = match stream.dequeue_buffer() {
        Some(buf) => buf,
        None => {
            warn!("Stream is out of buffers");
            return;
        }
    };

    let stride = std::mem::size_of::<f32>() * metadata.channels as usize;
    let datas = buffer.datas_mut();
    let data = &mut datas[0];

    let mut samples_written = 0;
    if let Some(slice) = data.data() {
        let slice = unsafe { &mut *(slice as *mut _ as *mut [f32]) };
        samples_written = (user_callback)(slice);
    }

    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = stride as _;
    *chunk.size_mut() = (stride * samples_written) as _;
}
