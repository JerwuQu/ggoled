use super::{Media, OSFeatures};
use mpris::{PlaybackStatus, PlayerFinder};

pub struct OSImpl {
    pf: Option<PlayerFinder>,
}
impl OSFeatures for OSImpl {
    fn new() -> Self {
        let pf = match PlayerFinder::new() {
            Ok(pf) => Some(pf),
            Err(err) => {
                eprintln!("failed to create MPRIS player finder: {err:?}");
                None
            }
        };
        Self { pf }
    }
    fn get_media(&mut self) -> Option<Media> {
        let pf = self.pf.as_ref()?;
        let player = pf.find_active().ok()?;
        let status = player.get_playback_status().ok()?;
        if !matches!(status, PlaybackStatus::Playing) {
            return None;
        }
        let meta = player.get_metadata().ok()?;
        let artists = meta.artists()?;
        let artist = artists.first()?;
        let title = meta.title()?;
        Some(Media {
            title: title.to_string(),
            artist: artist.to_string(),
        })
    }
    fn get_idle_seconds(&mut self) -> usize {
        // TODO
        0
    }
}
