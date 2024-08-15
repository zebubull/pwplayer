use symphonia::core::units::Time;

use crate::{command::Command, pw::PipewireLoopTx};

pub struct PlayerState {
    paused: bool,
    volume: Option<f32>,
    seek_request: Option<Time>,
    stream_tx: Option<PipewireLoopTx>,
}

impl PlayerState {
    pub fn new() -> Self {
        Self {
            paused: false,
            volume: Some(1.0),
            seek_request: None,
            stream_tx: None,
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
        self.volume = Some(volume);
    }

    pub fn get_volume(&mut self) -> Option<f32> {
        self.volume.take()
    }

    pub fn seek_to(&mut self, time: Time) {
        self.seek_request = Some(time);
    }

    pub fn get_seek(&mut self) -> Option<Time> {
        self.seek_request.take()
    }

    pub fn send(&self, command: Command) {
        if let Some(ref tx) = self.stream_tx {
            let _ = tx.send(command);
        }
    }

    pub fn update_tx(&mut self, tx: PipewireLoopTx) {
        self.stream_tx = Some(tx);
    }
}
