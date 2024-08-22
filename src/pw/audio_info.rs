use std::io::Cursor;

use pipewire::spa::{
    param::audio::{AudioFormat, AudioInfoRaw, MAX_CHANNELS},
    pod::{
        serialize::{GenError, PodSerializer},
        Object, Value,
    },
    sys::{
        SPA_PARAM_EnumFormat, SPA_TYPE_OBJECT_Format, SPA_AUDIO_CHANNEL_FL, SPA_AUDIO_CHANNEL_FR,
    },
};

pub struct AudioInfo {
    inner: AudioInfoRaw,
}

impl AudioInfo {
    pub fn new(rate: u32, channels: u32, format: AudioFormat) -> Self {
        assert_eq!(channels, 2, "Only 2 channel tracks are supported");
        let mut info = AudioInfoRaw::new();
        info.set_rate(rate);
        info.set_format(format);

        let mut positions = [0; MAX_CHANNELS];
        positions[0] = SPA_AUDIO_CHANNEL_FL;
        positions[1] = SPA_AUDIO_CHANNEL_FR;
        info.set_position(positions);

        Self { inner: info }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, GenError> {
        PodSerializer::serialize(
            Cursor::new(vec![]),
            &Value::Object(Object {
                type_: SPA_TYPE_OBJECT_Format,
                id: SPA_PARAM_EnumFormat,
                properties: self.inner.into(),
            }),
        )
        .map(|data| data.0.into_inner())
    }
}
