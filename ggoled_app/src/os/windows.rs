use super::Media;
use std::mem::size_of;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows_sys::Win32::{
    System::SystemInformation::GetTickCount,
    UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
};

pub struct MediaControl {
    mgr: Option<GlobalSystemMediaTransportControlsSessionManager>,
}
impl MediaControl {
    pub fn new() -> MediaControl {
        let mgr = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .map(|req| req.join().ok())
            .ok()
            .flatten();
        MediaControl { mgr }
    }
    pub fn get_media(&self) -> Option<Media> {
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
}

pub fn get_idle_seconds() -> usize {
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
