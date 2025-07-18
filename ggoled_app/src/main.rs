#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
compile_error!("ggoled_app can currently only be built for Windows");

mod os;

use anyhow::Context;
use chrono::{Local, TimeDelta, Timelike};
use ggoled_draw::{bitmap_from_memory, DrawDevice, DrawEvent, LayerId, ShiftMode, TextRenderer};
use ggoled_lib::Device;
use os::{dispatch_system_events, get_idle_seconds, Media, MediaControl};
use rfd::{MessageDialog, MessageLevel};
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, path::PathBuf, sync::Arc, thread::sleep, time::Duration};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    Icon, TrayIconBuilder,
};

const IDLE_TIMEOUT_SECS: usize = 60;
const NOTIF_DUR: Duration = Duration::from_secs(5);

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
    font: Option<ConfigFont>,
    show_time: bool,
    show_media: bool,
    idle_timeout: bool,
    oled_shift: ConfigShiftMode,
    show_notifications: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            font: None,
            show_time: true,
            show_media: true,
            idle_timeout: true,
            oled_shift: ConfigShiftMode::default(),
            show_notifications: true,
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

fn main() {
    // Initial loading
    let mut config = Config::load();
    let mut dev = DrawDevice::new(dialog_unwrap(Device::connect()), 30);
    if let Some(font) = &config.font {
        dev.texter = dialog_unwrap(TextRenderer::load_from_file(&font.path, font.size));
    }

    // Create tray icon with menu
    let tm_time_check = CheckMenuItem::new("Show time", true, config.show_time, None);
    let tm_media_check = CheckMenuItem::new("Show playing media", true, config.show_media, None);
    let tm_notif_check = CheckMenuItem::new("Show connection notifications", true, config.show_notifications, None);
    let tm_idle_check = CheckMenuItem::new("Screensaver when idle", true, config.idle_timeout, None);
    let tm_oledshift_off = CheckMenuItem::new("Off", true, false, None);
    let tm_oledshift_simple = CheckMenuItem::new("Simple", true, false, None);
    // TODO: remove all these closures and create a struct instead
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
        &tm_notif_check,
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
        // NOTE: `tray.set_icon(...)` can fail due to timeout in some conditions: ignore error
        _ = tray.set_icon(Some(
            (if con { &ggoled_normal_icon } else { &ggoled_error_icon }).clone(),
        ));
    };
    update_connection(true);
    update_oledshift(&mut dev, config.oled_shift);

    // Load icons
    let icon_hs_connect =
        Arc::new(bitmap_from_memory(include_bytes!("../assets/headset_connected.png"), 0x80).unwrap());
    let icon_hs_disconnect =
        Arc::new(bitmap_from_memory(include_bytes!("../assets/headset_disconnected.png"), 0x80).unwrap());

    // State
    let mgr = MediaControl::new();
    let menu_channel = MenuEvent::receiver();
    let mut last_time = Local::now() - TimeDelta::seconds(1);
    let mut last_media: Option<Media> = None;
    let mut time_layers: Vec<LayerId> = vec![];
    let mut media_layers: Vec<LayerId> = vec![];
    let mut notif_layer: Option<LayerId> = None;
    let mut notif_expiry = Local::now();
    let mut is_connected = None; // TODO: probe on startup

    // Go!
    dev.play();
    'main: loop {
        // Window event loop is required to get tray-icon working
        // TODO: handle system going to sleep
        // TDOO: context menu shouldn't freeze rendering
        dispatch_system_events();

        // Handle tray menu events
        let mut config_updated = false;
        while let Ok(event) = menu_channel.try_recv() {
            if event.id == tm_time_check.id() {
                config.show_time = tm_time_check.is_checked();
            } else if event.id == tm_media_check.id() {
                config.show_media = tm_media_check.is_checked();
            } else if event.id == tm_notif_check.id() {
                config.show_notifications = tm_notif_check.is_checked();
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

        let mut force_redraw = config_updated;

        // Handle events
        while let Some(event) = dev.try_event() {
            println!("event: {:?}", event);
            match event {
                DrawEvent::DeviceDisconnected => update_connection(false),
                DrawEvent::DeviceReconnected => update_connection(true),
                DrawEvent::DeviceEvent(event) => match event {
                    ggoled_lib::DeviceEvent::HeadsetConnection { wireless, .. } => {
                        if Some(wireless) != is_connected {
                            is_connected = Some(wireless);
                            if config.show_notifications {
                                if let Some(id) = notif_layer {
                                    dev.remove_layer(id);
                                }
                                notif_layer = Some(
                                    dev.add_layer(ggoled_draw::DrawLayer::Image {
                                        bitmap: (if wireless {
                                            &icon_hs_connect
                                        } else {
                                            &icon_hs_disconnect
                                        })
                                        .clone(),
                                        x: 8,
                                        y: 8,
                                    }),
                                );
                                notif_expiry = Local::now() + NOTIF_DUR;
                                force_redraw = true;
                            }
                        }
                    }
                    _ => {}
                },
            }
        }

        // Update layers every second
        let time = Local::now();
        if time.second() != last_time.second() || force_redraw {
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
                // TODO: perhaps notifications should be kept?
                dev.clear_layers(); // clear screen when idle
                last_media = None; // reset media so we check again when not idle
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
    let dev = dev.stop();
    dev.return_to_ui().unwrap();
}
