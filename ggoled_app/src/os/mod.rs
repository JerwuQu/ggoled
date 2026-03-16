#[derive(PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
}

pub trait OSFeatures {
    fn new() -> Self
    where
        Self: Sized;
    fn get_media(&mut self) -> Option<Media>;
    fn get_idle_seconds(&mut self) -> usize;
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::OSImpl;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::OSImpl;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::OSImpl;
