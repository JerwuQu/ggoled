use super::{Media, OSFeatures};
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
    fn get_idle_seconds(&mut self) -> usize {
        unsafe {
            let mut lastinput = LASTINPUTINFO {
                cbSize: size_of::<LASTINPUTINFO>() as u32,
                dwTime: 0,
            };
            if GetLastInputInfo(&mut lastinput) != 0 {
                ((GetTickCount() - lastinput.dwTime) / 1000) as usize
            } else {
                0
            }
        }
    }
}
