use symphonia::core::units::Time;

pub struct PlayerState {
    paused: bool,
    volume: f32,
    seek_request: Option<Time>,
}

impl PlayerState {
    pub fn new() -> Self {
        Self {
            paused: false,
            volume: 1.0,
            seek_request: None,
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

    pub fn seek_to(&mut self, time: Time) {
        self.seek_request = Some(time);
    }

    pub fn get_seek(&mut self) -> Option<Time> {
        self.seek_request.take()
    }
}
