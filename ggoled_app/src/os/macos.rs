use super::{Media, OSFeatures};
use media_remote::NowPlayingPerl;

pub struct OSImpl {
    npp: NowPlayingPerl,
}
impl OSFeatures for OSImpl {
    fn new() -> Self {
        let npp = NowPlayingPerl::new();
        Self { npp }
    }
    fn get_media(&mut self) -> Option<Media> {
        let guard = self.npp.get_info();
        let info = guard.as_ref()?;
        if info.is_playing == Some(true) {
            Some(Media {
                title: info.title.clone()?,
                artist: info.artist.clone()?,
            })
        } else {
            None
        }
    }
    fn get_idle_seconds(&mut self) -> usize {
        // TODO
        0
    }
}
