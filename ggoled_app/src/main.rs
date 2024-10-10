#![windows_subsystem = "windows"]

use chrono::{Local, TimeDelta, Timelike};
use ggoled_draw::{DrawDevice, LayerId};
use ggoled_lib::Device;
use media::{Media, MediaControl};
use serde::{Deserialize, Serialize};
use std::{mem::size_of, path::PathBuf, ptr::null_mut, thread::sleep, time::Duration};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    Icon, TrayIconBuilder,
};
use windows_sys::Win32::{
    System::{Console::AllocConsole, SystemInformation::GetTickCount},
    UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG},
    },
};
mod media;

const IDLE_TIMEOUT_SECS: usize = 60;

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Config {
    show_time: bool,
    show_media: bool,
    idle_timeout: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            show_time: true,
            show_media: true,
            idle_timeout: true,
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
        let Ok(text) = std::fs::read_to_string(&Self::path()) else {
            return Config::default();
        };
        let Ok(conf) = toml::from_str(&text) else {
            return Config::default();
        };
        conf
    }
}

fn main() {
    #[cfg(debug_assertions)]
    {
        unsafe { AllocConsole() };
    }

    let mut config = Config::load();

    let tm_time_check = CheckMenuItem::new("Show time", true, config.show_time, None);
    let tm_media_check = CheckMenuItem::new("Show playing media", true, config.show_media, None);
    let tm_idle_check = CheckMenuItem::new("Screensaver on idle", true, config.idle_timeout, None);
    let tm_quit = MenuItem::new("Quit", true, None);
    let tray_menu = Menu::with_items(&[
        &MenuItem::new("ggoled", false, None),
        &PredefinedMenuItem::separator(),
        &Submenu::with_items("Config", true, &[&tm_time_check, &tm_media_check, &tm_idle_check]).unwrap(),
        &PredefinedMenuItem::separator(),
        &tm_quit,
    ])
    .unwrap();
    let icon = {
        let icon_png = include_bytes!("../ggoled.png");
        let icon_rgba = image::load_from_memory(icon_png)
            .unwrap()
            .resize(32, 32, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        Icon::from_rgba(icon_rgba.to_vec(), icon_rgba.width(), icon_rgba.height()).unwrap()
    };
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("ggoled")
        .with_icon(icon)
        .build()
        .unwrap();

    let mut dev = DrawDevice::new(Device::connect().unwrap(), 30);
    dev.play();

    let mgr = MediaControl::new();

    let menu_channel = MenuEvent::receiver();
    let mut last_time = Local::now() - TimeDelta::seconds(1);
    let mut last_media: Option<Media> = None;
    let mut time_layers: Vec<LayerId> = vec![];
    let mut media_layers: Vec<LayerId> = vec![];
    'main: loop {
        // Window event loop is required to get tray-icon working
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, 1) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Handle tray menu events
        let mut config_updated = false;
        while let Ok(event) = menu_channel.try_recv() {
            if event.id == tm_time_check.id() {
                config.show_time = tm_time_check.is_checked();
                config_updated = true;
            } else if event.id == tm_media_check.id() {
                config.show_media = tm_media_check.is_checked();
                config_updated = true;
            } else if event.id == tm_idle_check.id() {
                config.idle_timeout = tm_idle_check.is_checked();
                config_updated = true;
            } else if event.id == tm_quit.id() {
                break 'main; // break main loop
            }
        }
        if config_updated {
            config.save().unwrap();
        }

        // Update layers every second
        let time = Local::now();
        if time.second() != last_time.second() || config_updated {
            last_time = time;

            // Check if idle
            let idle_seconds = unsafe {
                let mut lastinput = LASTINPUTINFO {
                    cbSize: size_of::<LASTINPUTINFO>() as u32,
                    dwTime: 0,
                };
                if GetLastInputInfo(&mut lastinput) != 0 {
                    ((GetTickCount() - lastinput.dwTime) / 1000) as usize
                } else {
                    0
                }
            };
            if config.idle_timeout && idle_seconds >= IDLE_TIMEOUT_SECS {
                dev.clear_layers(); // clear screen when idle
            } else {
                // Fetch media once a second (before pausing screen)
                let media = if config.show_media { mgr.get_media() } else { None };

                dev.pause();

                // Time
                dev.remove_layers(&time_layers);
                if config.show_time {
                    let time_str = time.format("%H:%M:%S").to_string();
                    time_layers = dev.add_text(&time_str, None, Some(8));
                }

                // Media
                if media != last_media {
                    dev.remove_layers(&media_layers);
                    if let Some(m) = &media {
                        media_layers = dev.add_text(
                            &format!("{}\n{}", m.title, m.artist),
                            None,
                            Some(8 + dev.font_line_height() as isize),
                        );
                    }
                    last_media = media;
                }

                dev.play();
            }
        }

        sleep(Duration::from_millis(10));
    }

    // Draw a blank frame when quitting
    dev.clear_layers();
    dev.await_frame();
}
