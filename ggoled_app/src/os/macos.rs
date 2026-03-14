use super::Media;
use media_remote::NowPlayingPerl;

pub struct MediaControl {
    npp: NowPlayingPerl,
}
impl MediaControl {
    pub fn new() -> MediaControl {
        let npp = NowPlayingPerl::new();
        MediaControl { npp }
    }
    pub fn get_media(&self) -> Option<Media> {
        let guard = self.npp.get_info();
        let info = guard.as_ref()?;
        Some(Media {
            title: info.title.clone()?,
            artist: info.artist.clone()?,
        })
    }
}

pub fn get_idle_seconds() -> usize {
    // TODO
    0
}
