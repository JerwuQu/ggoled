#![windows_subsystem = "windows"]

mod os;

use chrono::{Local, TimeDelta, Timelike};
use ggoled_draw::{DrawDevice, DrawEvent, LayerId, ShiftMode, TextRenderer};
use ggoled_lib::Device;
use os::{dispatch_system_events, get_idle_seconds, Media, MediaControl};
use rfd::{MessageDialog, MessageLevel};
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, path::PathBuf, thread::sleep, time::Duration};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    Icon, TrayIconBuilder,
};

const IDLE_TIMEOUT_SECS: usize = 60;

#[derive(Serialize, Deserialize, Default, Clone, Copy)]
enum ConfigShiftMode {
    Off,
    #[default]
    Simple,
}
impl ConfigShiftMode {
    fn to_api(self) -> ShiftMode {
        match self {
            ConfigShiftMode::Off => ShiftMode::Off,
            ConfigShiftMode::Simple => ShiftMode::Simple,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct ConfigFont {
    path: PathBuf,
    size: f32,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Config {
    show_time: bool,
    show_media: bool,
    idle_timeout: bool,
    oled_shift: ConfigShiftMode,
    font: Option<ConfigFont>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            show_time: true,
            show_media: true,
            idle_timeout: true,
            oled_shift: ConfigShiftMode::default(),
            font: None,
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

fn main() {
    #[cfg(debug_assertions)]
    {
        use windows_sys::Win32::System::Console::AllocConsole;
        unsafe { AllocConsole() };
    }

    // Initial loading
    let mut config = Config::load();
    let mut dev = DrawDevice::new(dialog_unwrap(Device::connect()), 30);
    if let Some(font) = &config.font {
        dev.texter = dialog_unwrap(TextRenderer::load_from_file(&font.path, font.size));
    }

    // Create tray icon with menu
    let tm_time_check = CheckMenuItem::new("Show time", true, config.show_time, None);
    let tm_media_check = CheckMenuItem::new("Show playing media", true, config.show_media, None);
    let tm_idle_check = CheckMenuItem::new("Screensaver when idle", true, config.idle_timeout, None);
    let tm_oledshift_off = CheckMenuItem::new("Off", true, false, None);
    let tm_oledshift_simple = CheckMenuItem::new("Simple", true, false, None);
    let update_oledshift = |dev: &mut DrawDevice, mode: ConfigShiftMode| {
        tm_oledshift_off.set_checked(matches!(mode, ConfigShiftMode::Off));
        tm_oledshift_simple.set_checked(matches!(mode, ConfigShiftMode::Simple));
        dev.set_shift_mode(mode.to_api());
    };
    let tm_quit = MenuItem::new("Quit", true, None);
    let tray_menu = dialog_unwrap(Menu::with_items(&[
        &MenuItem::new("ggoled", false, None),
        &PredefinedMenuItem::separator(),
        &tm_time_check,
        &tm_media_check,
        &tm_idle_check,
        &Submenu::with_items("OLED screen shift", true, &[&tm_oledshift_off, &tm_oledshift_simple]).unwrap(),
        &PredefinedMenuItem::separator(),
        &tm_quit,
    ]));

    let ggoled_normal_rgba = image::load_from_memory(include_bytes!("../assets/ggoled.png"))
        .unwrap()
        .resize(32, 32, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let ggoled_error_rgba = image::load_from_memory(include_bytes!("../assets/ggoled_error.png"))
        .unwrap()
        .resize(32, 32, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("ggoled")
        .build()
        .unwrap();

    let update_connection = |con: bool| {
        let rgba = if con { &ggoled_normal_rgba } else { &ggoled_error_rgba };
        tray.set_icon(Some(
            // NOTE: because tray-icon consumes the icon, we have to re-create it
            Icon::from_rgba(rgba.to_vec(), rgba.width(), rgba.height()).unwrap(),
        ))
        .unwrap();
    };
    update_connection(true);

    update_oledshift(&mut dev, config.oled_shift);
    dev.play();

    let mgr = MediaControl::new();

    let menu_channel = MenuEvent::receiver();
    let mut last_time = Local::now() - TimeDelta::seconds(1);
    let mut last_media: Option<Media> = None;
    let mut time_layers: Vec<LayerId> = vec![];
    let mut media_layers: Vec<LayerId> = vec![];
    'main: loop {
        // Window event loop is required to get tray-icon working
        dispatch_system_events();

        // Handle tray menu events
        let mut config_updated = false;
        while let Ok(event) = menu_channel.try_recv() {
            if event.id == tm_time_check.id() {
                config.show_time = tm_time_check.is_checked();
            } else if event.id == tm_media_check.id() {
                config.show_media = tm_media_check.is_checked();
            } else if event.id == tm_idle_check.id() {
                config.idle_timeout = tm_idle_check.is_checked();
            } else if event.id == tm_oledshift_off.id() {
                config.oled_shift = ConfigShiftMode::Off;
                update_oledshift(&mut dev, config.oled_shift);
            } else if event.id == tm_oledshift_simple.id() {
                config.oled_shift = ConfigShiftMode::Simple;
                update_oledshift(&mut dev, config.oled_shift);
            } else if event.id == tm_quit.id() {
                break 'main; // break main loop
            } else {
                continue; // no match, don't mark config as updated
            }
            config_updated = true;
        }
        if config_updated {
            dialog_unwrap(config.save());
        }

        // Handle events
        while let Some(event) = dev.try_event() {
            println!("event: {:?}", event);
            match event {
                DrawEvent::DeviceDisconnected => update_connection(false),
                DrawEvent::DeviceReconnected => update_connection(true),
                DrawEvent::DeviceEvent(event) => match event {
                    _ => {}
                },
            }
        }

        // Update layers every second
        let time = Local::now();
        if time.second() != last_time.second() || config_updated {
            last_time = time;

            // Check if idle
            let idle_seconds = get_idle_seconds();
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
                    time_layers = dev.add_text(&time_str, None, if media.is_some() { Some(8) } else { None });
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

                // TODO: show icon if headset was recently connected/disconnected

                dev.play();
            }
        }

        sleep(Duration::from_millis(10));
    }

    // Draw a blank frame when quitting
    dev.clear_layers();
}
