use super::Media;

pub struct MediaControl {}
impl MediaControl {
    pub fn new() -> MediaControl {
        MediaControl {}
    }
    pub fn get_media(&self) -> Option<Media> {
        todo!()
    }
}

pub fn dispatch_system_events() {
    todo!()
}

pub fn get_idle_seconds() -> usize {
    todo!()
}
