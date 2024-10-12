use super::Media;

pub struct MediaControl {}
impl MediaControl {
    pub fn new() -> MediaControl {
        MediaControl {}
    }
    pub fn get_media(&self) -> Option<Media> {
        None // TODO: use MPRIS
    }
}

pub fn dispatch_system_events() {
    // TODO
}

pub fn get_idle_seconds() -> usize {
    0 // TODO
}
