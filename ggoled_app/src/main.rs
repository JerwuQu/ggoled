#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
compile_error!("ggoled_app can currently only be built for Windows");

mod os;

use anyhow::Context;
use chrono::{Local, TimeDelta, Timelike};
use ggoled_draw::{bitmap_from_memory, DrawDevice, LayerId, ShiftMode, TextRenderer};
use ggoled_lib::Device;
use os::{dispatch_system_events, get_idle_seconds, MediaControl};
use rfd::{MessageDialog, MessageLevel};
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, path::PathBuf, sync::Arc, sync::mpsc::{channel, Receiver, Sender}, time::Duration};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    Icon, TrayIconBuilder,
};

const IDLE_TIMEOUT_SECS: usize = 60;

#[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq)]
enum ConfigShiftMode {
    Off,
    #[default]
    Simple,
}
impl ConfigShiftMode {
    #[allow(dead_code)]
    fn to_api(self) -> ShiftMode {
        match self {
            ConfigShiftMode::Off => ShiftMode::Off,
            ConfigShiftMode::Simple => ShiftMode::Simple,
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct ConfigFont {
    path: PathBuf,
    size: f32,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Config {
    font: Option<ConfigFont>,
    show_time: bool,
    show_media: bool,
    show_cover: bool,
    idle_timeout: bool,
    oled_shift: ConfigShiftMode,
    show_notifications: bool,
    ignore_browser_media: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            font: None,
            show_time: true,
            show_media: true,
            show_cover: true,
            idle_timeout: true,
            oled_shift: ConfigShiftMode::default(),
            show_notifications: true,
            ignore_browser_media: true,
        }
    }
}
impl Config {
    fn path() -> PathBuf {
        directories::BaseDirs::new()
            .unwrap()
            .config_dir()
            .join("ggoled_app.toml")
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let text = toml::to_string(self)?;
        std::fs::write(Self::path(), text)?;
        Ok(())
    }
    pub fn load() -> Config {
        let Ok(text) = std::fs::read_to_string(Self::path()) else {
            return Config::default();
        };
        let Ok(conf) = toml::from_str(&text) else {
            return Config::default();
        };
        conf
    }
}

// unwrap an error and show a MessageDialog if it fails
pub fn dialog_unwrap<T, E: Debug>(res: Result<T, E>) -> T {
    match res {
        Ok(v) => v,
        Err(e) => {
            let str = format!("Error: {:?}", e);
            MessageDialog::new()
                .set_level(MessageLevel::Error)
                .set_title("ggoled")
                .set_description(&str)
                .show();
            panic!("dialog_unwrap: {}", str);
        }
    }
}

fn load_icon(buf: &[u8]) -> Icon {
    Icon::from_rgba(
        image::load_from_memory(buf)
            .unwrap()
            .resize(32, 32, image::imageops::FilterType::Lanczos3)
            .to_rgba8()
            .to_vec(),
        32,
        32,
    )
    .unwrap()
}

fn is_cover_useless(bitmap: &ggoled_lib::Bitmap) -> bool {
    let total_pixels = bitmap.data.len();
    if total_pixels == 0 {
        return true;
    }
    
    let mut dark_pixels = 0;
    for i in 0..total_pixels {
        if !bitmap.data[i] {
            dark_pixels += 1;
        }
    }
    
    // Consider cover useless if 90% or more pixels are dark
    (dark_pixels as f32 / total_pixels as f32) >= 0.9
}

fn resize_bitmap(bitmap: &ggoled_lib::Bitmap, target_width: usize, target_height: usize) -> ggoled_lib::Bitmap {
    let mut resized = ggoled_lib::Bitmap::new(target_width, target_height, false);
    
    for y in 0..target_height {
        for x in 0..target_width {
            let src_x = (x * bitmap.w) / target_width;
            let src_y = (y * bitmap.h) / target_height;
            
            let src_idx = src_y * bitmap.w + src_x;
            let dst_idx = y * target_width + x;
            
            if src_idx < bitmap.data.len() && dst_idx < resized.data.len() {
                resized.data.set(dst_idx, bitmap.data[src_idx]);
            }
        }
    }
    
    resized
}

// Messages sent to OLED worker thread
enum WorkerMsg {
    UpdateConfig(Config),
    Quit,
}

// Messages sent from worker to main thread
#[allow(dead_code)]
enum MainMsg {
    // Currently unused, extensible for future use
}

fn main() {
    let mut config = Config::load();
    let (tx_worker, rx_worker): (Sender<WorkerMsg>, Receiver<WorkerMsg>) = channel();
    let (_tx_main, _rx_main): (Sender<MainMsg>, Receiver<MainMsg>) = channel();

    // Launch OLED worker thread
    let config_clone = config.clone();
    let worker_handle = std::thread::spawn(move || {
        oled_worker(rx_worker, _tx_main, config_clone);
    });

    // Create tray icon with menu
    let tm_time_check = CheckMenuItem::new("Show time", true, config.show_time, None);
    let tm_media_check = CheckMenuItem::new("Show playing media", true, config.show_media, None);
    let tm_cover_check = CheckMenuItem::new("Show album covers", true, config.show_cover, None);
    let tm_notif_check = CheckMenuItem::new("Show connection notifications", true, config.show_notifications, None);
    let tm_ignore_browser_check = CheckMenuItem::new("Ignore browser media", true, config.ignore_browser_media, None);
    let tm_idle_check = CheckMenuItem::new("Screensaver when idle", true, config.idle_timeout, None);
    let tm_oledshift_off = CheckMenuItem::new("Off", true, config.oled_shift == ConfigShiftMode::Off, None);
    let tm_oledshift_simple = CheckMenuItem::new("Simple", true, config.oled_shift == ConfigShiftMode::Simple, None);
    let tm_quit = MenuItem::new("Quit", true, None);
    let tray_menu = dialog_unwrap(Menu::with_items(&[
        &MenuItem::new("ggoled", false, None),
        &PredefinedMenuItem::separator(),
        &tm_time_check,
        &tm_media_check,
        &tm_cover_check,
        &tm_notif_check,
        &tm_ignore_browser_check,
        &tm_idle_check,
        &Submenu::with_items("OLED screen shift", true, &[&tm_oledshift_off, &tm_oledshift_simple]).unwrap(),
        &PredefinedMenuItem::separator(),
        &tm_quit,
    ]));

    let ggoled_normal_icon = load_icon(include_bytes!("../assets/ggoled.png"));
    let ggoled_error_icon = load_icon(include_bytes!("../assets/ggoled_error.png"));
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("ggoled")
        .build()
        .context("Failed to create tray icon")
        .unwrap();

    let update_connection = |con: bool| {
        _ = tray.set_icon(Some(
            (if con { &ggoled_normal_icon } else { &ggoled_error_icon }).clone(),
        ));
    };
    update_connection(true);

    let menu_channel = MenuEvent::receiver();

    'main: loop {
        // Process system events quickly to prevent menu blocking
        let start_event_time = std::time::Instant::now();
        while start_event_time.elapsed() < std::time::Duration::from_millis(5) {
            dispatch_system_events();
        }

        // Handle tray menu events
        let mut config_updated = false;
        while let Ok(event) = menu_channel.try_recv() {
            if event.id == tm_time_check.id() {
                config.show_time = tm_time_check.is_checked();
            } else if event.id == tm_media_check.id() {
                config.show_media = tm_media_check.is_checked();
            } else if event.id == tm_cover_check.id() {
                config.show_cover = tm_cover_check.is_checked();
            } else if event.id == tm_notif_check.id() {
                config.show_notifications = tm_notif_check.is_checked();
            } else if event.id == tm_ignore_browser_check.id() {
                config.ignore_browser_media = tm_ignore_browser_check.is_checked();
            } else if event.id == tm_idle_check.id() {
                config.idle_timeout = tm_idle_check.is_checked();
            } else if event.id == tm_oledshift_off.id() {
                config.oled_shift = ConfigShiftMode::Off;
                tm_oledshift_off.set_checked(true);
                tm_oledshift_simple.set_checked(false);
            } else if event.id == tm_oledshift_simple.id() {
                config.oled_shift = ConfigShiftMode::Simple;
                tm_oledshift_off.set_checked(false);
                tm_oledshift_simple.set_checked(true);
            } else if event.id == tm_quit.id() {
                let _ = tx_worker.send(WorkerMsg::Quit);
                break 'main;
            } else {
                continue;
            }
            config_updated = true;
        }
        if config_updated {
            dialog_unwrap(config.save());
            let _ = tx_worker.send(WorkerMsg::UpdateConfig(config.clone()));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Wait for worker thread to finish
    let _ = worker_handle.join();
}

fn oled_worker(rx: Receiver<WorkerMsg>, _tx: Sender<MainMsg>, mut config: Config) {
    // Initialize OLED display and media control
    let mut dev = DrawDevice::new(dialog_unwrap(Device::connect()), 30);
    if let Some(font) = &config.font {
        dev.texter = dialog_unwrap(TextRenderer::load_from_file(&font.path, font.size));
    }
    
    // Configure OLED pixel shift to prevent burn-in
    dev.set_shift_mode(config.oled_shift.to_api());
    
    let mut mgr = MediaControl::new();
    let icon_hs_connect = Arc::new(bitmap_from_memory(include_bytes!("../assets/headset_connected.png"), 0x80).unwrap());
    let icon_hs_disconnect = Arc::new(bitmap_from_memory(include_bytes!("../assets/headset_disconnected.png"), 0x80).unwrap());
    let mut last_time = Local::now() - TimeDelta::seconds(1);
    let mut last_media_info = String::new();
    let mut last_config_state = format!("{}{}{}", config.show_time, config.show_media, config.show_cover);
    let mut time_layers: Vec<LayerId> = vec![];
    let mut media_layers: Vec<LayerId> = vec![];
    let mut cover_layer: Option<LayerId> = None;
    let mut notif_layer: Option<LayerId> = None;
    let mut notif_expiry = Local::now();
    dev.play();
    loop {
        // Check for messages from main thread
        while let Ok(msg) = rx.try_recv() {
            match msg {
                WorkerMsg::UpdateConfig(new_conf) => {
                    // Update OLED shift mode if it changed
                    if config.oled_shift != new_conf.oled_shift {
                        dev.set_shift_mode(new_conf.oled_shift.to_api());
                    }
                    config = new_conf;
                }
                WorkerMsg::Quit => {
                    let dev = dev.stop();
                    let _ = dev.return_to_ui();
                    return;
                }
            }
        }

        // Handle device events for notifications
        while let Some(event) = dev.try_event() {
            match event {
                ggoled_draw::DrawEvent::DeviceDisconnected => {}
                ggoled_draw::DrawEvent::DeviceReconnected => {}
                ggoled_draw::DrawEvent::DeviceEvent(device_event) => match device_event {
                    ggoled_lib::DeviceEvent::HeadsetConnection { connected } => {
                        // Only show notification if not in cover + media mode
                        let media = if config.show_media { mgr.get_media(config.ignore_browser_media) } else { None };
                        let should_show_cover = config.show_cover && config.show_media && media.is_some();
                        let has_usable_cover = if let Some(m) = &media {
                            m.cover.as_ref().map_or(false, |cover| !is_cover_useless(cover))
                        } else {
                            false
                        };
                        let display_cover = should_show_cover && has_usable_cover;
                        
                        if config.show_notifications && !display_cover {
                            if let Some(id) = notif_layer {
                                dev.remove_layer(id);
                            }
                            notif_layer = Some(
                                dev.add_layer(ggoled_draw::DrawLayer::Image {
                                    bitmap: (if connected {
                                        &icon_hs_connect
                                    } else {
                                        &icon_hs_disconnect
                                    })
                                    .clone(),
                                    x: 8,
                                    y: 8,
                                }),
                            );
                            notif_expiry = Local::now() + Duration::from_secs(5);
                        }
                    }
                    ggoled_lib::DeviceEvent::Volume { .. } => {}
                    ggoled_lib::DeviceEvent::Battery { .. } => {}
                }
            }
        }

        // Update layers every second
        let time = Local::now();
        if time.second() != last_time.second() {
            last_time = time;

            // Remove expired notifications
            if let Some(id) = notif_layer {
                if time >= notif_expiry {
                    dev.remove_layer(id);
                    notif_layer = None;
                }
            }

            // Check if idle
            let idle_seconds = get_idle_seconds();
            if config.idle_timeout && idle_seconds >= IDLE_TIMEOUT_SECS {
                dev.clear_layers();
                cover_layer = None;
            } else {
                // Fetch media information
                let media = if config.show_media { mgr.get_media(config.ignore_browser_media) } else { None };

                // Check if display needs updating
                let current_media_info = media.as_ref().map_or(String::new(), |m| format!("{}_{}", m.title, m.artist));
                let current_config_state = format!("{}{}{}", config.show_time, config.show_media, config.show_cover);
                let media_changed = current_media_info != last_media_info;
                let config_changed = current_config_state != last_config_state;
                let need_update = media_changed || config_changed;                if need_update {
                    dev.pause();

                    // Clear existing layers to prevent overlap
                    dev.remove_layers(&time_layers);
                    dev.remove_layers(&media_layers);
                    if let Some(id) = cover_layer {
                        dev.remove_layer(id);
                        cover_layer = None;
                    }
                    
                    // Determine layout based on cover availability
                    let should_show_cover = config.show_cover && config.show_media && media.is_some();
                    let has_usable_cover = if let Some(m) = &media {
                        m.cover.as_ref().map_or(false, |cover| !is_cover_useless(cover))
                    } else {
                        false
                    };
                    let display_cover = should_show_cover && has_usable_cover;
                    
                    // Remove notification if now displaying cover + media
                    if display_cover && notif_layer.is_some() {
                        if let Some(id) = notif_layer {
                            dev.remove_layer(id);
                            notif_layer = None;
                        }
                    }
                    
                    if display_cover {
                        // Layout with cover: compact time and media on left, cover on right
                        if let Some(m) = &media {
                            if let Some(cover_bitmap) = &m.cover {
                                let resized_cover = std::sync::Arc::new(resize_bitmap(cover_bitmap, 44, 44));
                                cover_layer = Some(dev.add_layer(ggoled_draw::DrawLayer::Image {
                                    bitmap: resized_cover,
                                    x: 82, // Right position
                                    y: 10, // Centered vertically
                                }));
                            }
                        }
                        
                        if config.show_time {
                            let time_str = time.format("%H:%M:%S").to_string();
                            time_layers = dev.add_text_with_max_width(
                                &time_str,
                                Some(0),
                                Some(8),
                                Some(80), // Limited width for cover space
                            );
                        }
                        
                        if let Some(m) = &media {
                            media_layers = dev.add_text_with_max_width(
                                &format!("{}\n{}", m.title, m.artist),
                                Some(0),
                                Some(8 + dev.font_line_height() as isize),
                                Some(80), // Limited width for cover space
                            );
                        }
                    } else {
                        // Layout without cover: full screen elements
                        if media.is_some() && config.show_media {
                            // Media playing: small time at top, media centered
                            if config.show_time {
                                let time_str = time.format("%H:%M:%S").to_string();
                                time_layers = dev.add_text_with_max_width(
                                    &time_str,
                                    None, // Center horizontally
                                    Some(2), // Top position
                                    None, // Full width available
                                );
                            }
                            
                            if let Some(m) = &media {
                                media_layers = dev.add_text_with_max_width(
                                    &format!("{}\n{}", m.title, m.artist),
                                    None, // Center horizontally
                                    Some(8 + dev.font_line_height() as isize + 8),
                                    None, // Full width available
                                );
                            }
                        } else {
                            // No media: large centered time
                            if config.show_time {
                                let time_str = time.format("%H:%M:%S").to_string();
                                let original_size = dev.texter.get_size();
                                dev.texter.set_size(24.0); // Larger font for fullscreen
                                
                                // Calculate right-shifted position for notification space
                                let time_bitmap = dev.texter.render_lines(&time_str);
                                let time_width = if !time_bitmap.is_empty() { time_bitmap[0].w } else { 0 };
                                let center_x = (128 - time_width) / 2;
                                let shifted_x = center_x + 8; // Shift right for notification space
                                
                                time_layers = dev.add_text_with_max_width(
                                    &time_str,
                                    Some(shifted_x as isize),
                                    None, // Center vertically
                                    None,
                                );
                                dev.texter.set_size(original_size);
                            }
                        }
                    }
                    
                    // Update tracking states
                    last_media_info = current_media_info;
                    last_config_state = current_config_state;
                    
                    dev.play();
                } else if config.show_time {
                    // Update only time display to preserve scrolling
                    dev.pause();
                    dev.remove_layers(&time_layers);
                    
                    let display_cover = config.show_cover && config.show_media && media.is_some() &&
                        media.as_ref().unwrap().cover.as_ref().map_or(false, |cover| !is_cover_useless(cover));
                    
                    if display_cover || (media.is_some() && config.show_media) {
                        // Compact time layout
                        let time_str = time.format("%H:%M:%S").to_string();
                        time_layers = dev.add_text_with_max_width(
                            &time_str,
                            if display_cover { Some(0) } else { None },
                            Some(if display_cover { 8 } else { 2 }),
                            if display_cover { Some(80) } else { None },
                        );
                    } else {
                        // Large fullscreen time
                        let time_str = time.format("%H:%M:%S").to_string();
                        let original_size = dev.texter.get_size();
                        dev.texter.set_size(24.0);
                        
                        let time_bitmap = dev.texter.render_lines(&time_str);
                        let time_width = if !time_bitmap.is_empty() { time_bitmap[0].w } else { 0 };
                        let center_x = (128 - time_width) / 2;
                        let shifted_x = center_x + 8;
                        
                        time_layers = dev.add_text_with_max_width(
                            &time_str,
                            Some(shifted_x as isize),
                            None,
                            None,
                        );
                        dev.texter.set_size(original_size);
                    }
                    
                    dev.play();
                }

                dev.play();
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
