use symphonia::core::units::Time;

use crate::{command::Command, pw::PipewireLoopTx};

pub struct PlayerState {
    seek_request: Option<Time>,
    stream_tx: Option<PipewireLoopTx>,
}

impl PlayerState {
    pub fn new() -> Self {
        Self {
            seek_request: None,
            stream_tx: None,
        }
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
