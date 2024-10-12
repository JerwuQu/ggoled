use super::Media;
use std::{mem::size_of, ptr::null_mut};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows_sys::Win32::{
    System::SystemInformation::GetTickCount,
    UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG},
    },
};

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

pub fn dispatch_system_events() {
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, null_mut(), 0, 0, 1) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
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
