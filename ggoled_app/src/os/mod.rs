#[derive(PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
}

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(not(target_os = "windows"))]
pub mod linux;
#[cfg(not(target_os = "windows"))]
pub use linux::*;
