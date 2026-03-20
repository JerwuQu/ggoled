use super::{Media, OSFeatures};
use mpris::{PlaybackStatus, PlayerFinder};

mod wayland;
use wayland::IdleTracker;

pub struct OSImpl {
    pf: Option<PlayerFinder>,
    idle_tracker: Option<IdleTracker>,
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
        let idle_tracker = IdleTracker::new();
        if idle_tracker.is_none() {
            eprintln!("failed to init wayland idle tracker");
        }
        Self { pf, idle_tracker }
    }

    fn supports_media(&self) -> bool {
        self.pf.is_some()
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

    fn supports_idle(&self) -> bool {
        self.idle_tracker.is_some()
    }
    fn is_idle(&mut self) -> bool {
        let Some(tracker) = self.idle_tracker.as_mut() else {
            return false;
        };
        match tracker.get_idle() {
            Ok(idle) => idle,
            Err(err) => {
                eprintln!("wayland idle tracker failed: {err}");
                self.idle_tracker = None;
                false
            }
        }
    }
}
