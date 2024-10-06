use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};

#[derive(PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
}
pub struct MediaControl {
    mgr: GlobalSystemMediaTransportControlsSessionManager,
}
impl MediaControl {
    pub fn new() -> MediaControl {
        let request = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().unwrap();
        let mgr = request.get().unwrap();
        MediaControl { mgr }
    }
    pub fn get_media(&self) -> Option<Media> {
        (|| {
            let session = self.mgr.GetCurrentSession()?;
            let playing = session.GetPlaybackInfo()?.PlaybackStatus()?
                == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
            if playing {
                let request = session.TryGetMediaPropertiesAsync()?;
                let media = request.get().unwrap();
                anyhow::Ok(Some(Media {
                    title: media.Title()?.to_string_lossy(),
                    artist: media.Artist()?.to_string_lossy(),
                }))
            } else {
                anyhow::Ok(None)
            }
        })()
        .ok()
        .flatten()
    }
}
