use super::Media;
use std::{mem::size_of, ptr::null_mut, sync::Arc, time::{Duration, Instant}, collections::HashMap};
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
    cover_cache: HashMap<String, Arc<ggoled_lib::Bitmap>>,
    failed_covers: std::collections::HashSet<String>,
    last_cover_attempt: HashMap<String, Instant>,
}

impl MediaControl {
    pub fn new() -> MediaControl {
        let request = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().unwrap();
        let mgr = request.get().unwrap();
        
        MediaControl { 
            mgr,
            cover_cache: HashMap::new(),
            failed_covers: std::collections::HashSet::new(),
            last_cover_attempt: HashMap::new(),
        }
    }

    pub fn get_media(&mut self, ignore_browser_media: bool) -> Option<Media> {
        // Get all active sessions
        let sessions = self.mgr.GetSessions().ok()?;
        
        // Find the first session that's playing (and optionally non-browser)
        for session in sessions {
            let playing = session.GetPlaybackInfo().ok()?.PlaybackStatus().ok()?
                == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
            
            if !playing {
                continue;
            }
            
            // Check if this is a browser (only if ignore_browser_media is true)
            if ignore_browser_media {
                if let Ok(source_app_info) = session.SourceAppUserModelId() {
                    let source_id = source_app_info.to_string_lossy().to_lowercase();
                    
                    // Filter out common browsers
                    if source_id.contains("chrome") || 
                       source_id.contains("firefox") || 
                       source_id.contains("edge") || 
                       source_id.contains("opera") || 
                       source_id.contains("brave") || 
                       source_id.contains("safari") ||
                       source_id.contains("msedge") ||
                       source_id.contains("vivaldi") {
                        continue; // Skip browser sessions
                    }
                }
            }

            let request = session.TryGetMediaPropertiesAsync().ok()?;
            let media = request.get().ok()?;
            
            let title = media.Title().ok()?.to_string_lossy();
            let artist = media.Artist().ok()?.to_string_lossy();
            let cache_key = format!("{}_{}", title, artist);

            // Check cache first
            if let Some(cached_cover) = self.cover_cache.get(&cache_key) {
                return Some(Media {
                    title,
                    artist,
                    cover: Some(cached_cover.clone()),
                });
            }

            // Check if we've failed this cover before
            if self.failed_covers.contains(&cache_key) {
                return Some(Media {
                    title,
                    artist,
                    cover: None,
                });
            }

            // Check if we tried recently (avoid spamming)
            if let Some(last_attempt) = self.last_cover_attempt.get(&cache_key) {
                if last_attempt.elapsed() < Duration::from_secs(5) {
                    return Some(Media {
                        title,
                        artist,
                        cover: None,
                    });
                }
            }

            // Try to load cover with strict timeout
            self.last_cover_attempt.insert(cache_key.clone(), Instant::now());
            let cover = self.try_load_cover_fast(&media, &cache_key);

            return Some(Media {
                title,
                artist,
                cover,
            });
        }
        
        None // No sessions found (or no non-browser sessions if filtering is enabled)
    }

    fn try_load_cover_fast(&mut self, media: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties, cache_key: &str) -> Option<Arc<ggoled_lib::Bitmap>> {
        let start_time = Instant::now();
        
        // Get thumbnail
        let thumbnail = match media.Thumbnail() {
            Ok(thumb) => thumb,
            Err(_) => {
                self.failed_covers.insert(cache_key.to_string());
                return None;
            }
        };
        
        if start_time.elapsed() > Duration::from_millis(10) {
            self.failed_covers.insert(cache_key.to_string());
            return None;
        }

        // Open stream
        let stream_async = match thumbnail.OpenReadAsync() {
            Ok(async_op) => async_op,
            Err(_) => {
                self.failed_covers.insert(cache_key.to_string());
                return None;
            }
        };
        
        if start_time.elapsed() > Duration::from_millis(50) {
            self.failed_covers.insert(cache_key.to_string());
            return None;
        }
        
        // Get stream result
        let stream = match stream_async.get() {
            Ok(s) => s,
            Err(_) => {
                self.failed_covers.insert(cache_key.to_string());
                return None;
            }
        };
        
        if start_time.elapsed() > Duration::from_millis(100) {
            self.failed_covers.insert(cache_key.to_string());
            return None;
        }
        
        // Get size
        let size = match stream.Size() {
            Ok(s) => s,
            Err(_) => {
                self.failed_covers.insert(cache_key.to_string());
                return None;
            }
        };
        
        if size == 0 || size > 2_000_000 {
            self.failed_covers.insert(cache_key.to_string());
            return None;
        }
        
        if start_time.elapsed() > Duration::from_millis(150) {
            self.failed_covers.insert(cache_key.to_string());
            return None;
        }

        // Create buffer
        let buffer = match windows::Storage::Streams::Buffer::Create(size as u32) {
            Ok(buf) => buf,
            Err(_) => {
                self.failed_covers.insert(cache_key.to_string());
                return None;
            }
        };
        
        // Read async
        let read_async = match stream.ReadAsync(&buffer, size as u32, windows::Storage::Streams::InputStreamOptions::None) {
            Ok(async_op) => async_op,
            Err(_) => {
                self.failed_covers.insert(cache_key.to_string());
                return None;
            }
        };
        
        if start_time.elapsed() > Duration::from_millis(200) {
            self.failed_covers.insert(cache_key.to_string());
            return None;
        }
        
        // Get read result
        let read_buffer = match read_async.get() {
            Ok(buf) => buf,
            Err(_) => {
                self.failed_covers.insert(cache_key.to_string());
                return None;
            }
        };
        
        if start_time.elapsed() > Duration::from_millis(300) {
            self.failed_covers.insert(cache_key.to_string());
            return None;
        }

        // Read data
        let data_reader = match windows::Storage::Streams::DataReader::FromBuffer(&read_buffer) {
            Ok(reader) => reader,
            Err(_) => {
                self.failed_covers.insert(cache_key.to_string());
                return None;
            }
        };
        
        let mut bytes = vec![0u8; size as usize];
        if data_reader.ReadBytes(&mut bytes).is_err() {
            self.failed_covers.insert(cache_key.to_string());
            return None;
        }

        // Convert to bitmap
        let bitmap = match ggoled_draw::bitmap_from_memory(&bytes, 128) {
            Ok(bmp) => Arc::new(bmp),
            Err(_) => {
                self.failed_covers.insert(cache_key.to_string());
                return None;
            }
        };

        // Cache successful result
        self.cover_cache.insert(cache_key.to_string(), bitmap.clone());
        Some(bitmap)
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


