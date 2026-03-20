use super::{IDLE_TIMEOUT_MS, Media, OSFeatures};
use std::mem::size_of;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows_sys::Win32::{
    System::SystemInformation::GetTickCount,
    UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
};

pub struct OSImpl {
    mgr: Option<GlobalSystemMediaTransportControlsSessionManager>,
}
impl OSFeatures for OSImpl {
    fn new() -> Self {
        let mgr = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .map(|req| req.join().ok())
            .ok()
            .flatten();
        Self { mgr }
    }

    fn supports_media(&self) -> bool {
        self.mgr.is_some()
    }
    fn get_media(&mut self) -> Option<Media> {
        if let Some(mgr) = &self.mgr {
            (|| {
                let session = mgr.GetCurrentSession()?;
                let playing = session.GetPlaybackInfo()?.PlaybackStatus()?
                    == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
                if playing {
                    let request = session.TryGetMediaPropertiesAsync()?;
                    let media = request.join()?;
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
        } else {
            None
        }
    }

    fn supports_idle(&self) -> bool {
        true
    }
    fn is_idle(&mut self) -> bool {
        unsafe {
            let mut lastinput = LASTINPUTINFO {
                cbSize: size_of::<LASTINPUTINFO>() as u32,
                dwTime: 0,
            };
            GetLastInputInfo(&mut lastinput) != 0 && GetTickCount().wrapping_sub(lastinput.dwTime) >= IDLE_TIMEOUT_MS
        }
    }
}
