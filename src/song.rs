use std::{error::Error, fs::File, path::Path};
use symphonia::{
    core::{
        audio::RawSampleBuffer,
        codecs::Decoder,
        errors::Error as SymphoniaError,
        formats::{FormatReader, SeekMode, SeekTo},
        io::MediaSourceStream,
        meta::StandardTagKey,
        probe::Hint,
        units::Time,
    },
    default,
};

pub struct SongReader {
    buffer: Option<RawSampleBuffer<f32>>,
    pub channels: u32,
    decoder: Box<dyn Decoder>,
    pub rate: u32,
    reader: Box<dyn FormatReader>,
    track_id: u32,
    pub name: Option<String>,
}

impl SongReader {
    pub fn from_file<T: AsRef<Path>>(path: T) -> Result<Self, Box<dyn Error>> {
        let codecs = default::get_codecs();
        let probe = default::get_probe();

        let stream =
            MediaSourceStream::new(Box::new(File::open(path.as_ref())?), Default::default());
        let mut probed = probe.format(
            &Hint::default(),
            stream,
            &Default::default(),
            &Default::default(),
        )?;

        let name = if let Some(md) = probed.metadata.get().as_ref().and_then(|m| m.current()) {
            let mut name = None;
            for (_i, tag) in md.tags().iter().enumerate() {
                // println!("[{:0>2}] {: <20} : {}", i, tag.key, tag.value);
                let _ = tag.std_key.and_then(|k| {
                    if k == StandardTagKey::TrackTitle {
                        name = Some(tag.value.to_string());
                    }
                    Some(())
                });
            }
            name
        } else {
            None
        };

        let reader = probed.format;

        let track = reader.default_track().ok_or("File has no tracks")?;
        let decoder = codecs.make(&track.codec_params, &Default::default())?;
        let track_id = track.id;

        let params = &track.codec_params;
        let channels = params.channels.as_ref().ok_or("No channel data")?.count() as u32;
        let rate = params.sample_rate.ok_or("No sample rate")?;

        Ok(Self {
            buffer: None,
            channels,
            decoder,
            rate,
            reader,
            track_id,
            name,
        })
    }

    pub fn next_chunk(&mut self) -> Result<&RawSampleBuffer<f32>, Box<dyn Error>> {
        let packet = match self.reader.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::ResetRequired) => {
                self.decoder.reset();
                return self.next_chunk();
            }
            Err(e) => Err(e)?,
        };

        let decoded = self.decoder.decode(&packet)?;

        if self.buffer.is_none() {
            let buffer = RawSampleBuffer::new(decoded.capacity() as u64, *decoded.spec());
            let _ = self.buffer.replace(buffer);
        }

        let buffer = self.buffer.as_mut().unwrap();
        buffer.copy_interleaved_ref(decoded);

        Ok(buffer)
    }

    pub fn seek_time(&mut self, time: Time) -> Result<(), Box<dyn Error>> {
        self.reader.seek(
            SeekMode::Coarse,
            SeekTo::Time {
                time,
                track_id: Some(self.track_id),
            },
        )?;

        Ok(())
    }
}
