use std::{
    ffi::{c_void, OsString},
    os::windows::ffi::OsStringExt,
};

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW};

#[derive(PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
}
pub struct MediaControl {
    mgr: GlobalSystemMediaTransportControlsSessionManager,
}
impl MediaControl {
    pub fn new() -> anyhow::Result<MediaControl> {
        let request = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?;
        let mgr = request.get()?;
        Ok(MediaControl { mgr })
    }
    pub fn get_media(&self) -> Option<Media> {
        (|| {
            let session = self.mgr.GetCurrentSession()?;
            let playing = session.GetPlaybackInfo()?.PlaybackStatus()?
                == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
            if playing {
                let request = session.TryGetMediaPropertiesAsync()?;
                let media = request.get()?;
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
    pub fn get_from_window_titles(&self, base: Option<&Media>) -> Option<Media> {
        // TODO: use CreateToolhelp32Snapshot to get processes
        // TODO: find window for process
        // TODO: *cache process and window id to avoid constant lookups*
        unsafe extern "system" fn enumerate(hwnd: *mut c_void, _: isize) -> i32 {
            let mut buf = [0u16; 4096];
            let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 4096) as usize;
            let text = OsString::from_wide(&buf[..len]);
            1
        }
        unsafe { EnumWindows(Some(enumerate), 0) };
        todo!() //
    }
}
