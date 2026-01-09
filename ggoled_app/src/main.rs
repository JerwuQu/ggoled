#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod os;

use chrono::{Local, TimeDelta, Timelike};
use ggoled_draw::{bitmap_from_memory, DrawDevice, DrawEvent, LayerId, ShiftMode, TextRenderer};
use ggoled_lib::Device;
use os::{get_idle_seconds, Media, MediaControl};
use rfd::{MessageDialog, MessageLevel};
use sdl3_sys::everything as sdl;
use serde::{Deserialize, Serialize};
use std::{
    ffi::CStr,
    fmt::Debug,
    os::raw::c_void,
    path::PathBuf,
    sync::{mpsc, Arc},
    thread::sleep,
    time::Duration,
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

struct Icon {
    _pixels: Vec<u8>,
    surf: *mut sdl::SDL_Surface,
}
impl Icon {
    fn load(buf: &[u8]) -> Self {
        let pixels = image::load_from_memory(buf)
            .unwrap()
            .resize(32, 32, image::imageops::FilterType::Lanczos3)
            .to_rgba8()
            .into_vec();
        let surf = unsafe {
            sdl::SDL_CreateSurfaceFrom(
                32,
                32,
                sdl::SDL_PixelFormat::RGBA32,
                pixels.as_ptr() as *mut c_void,
                32 * 4,
            )
        };
        assert!(!surf.is_null());
        Self { _pixels: pixels, surf }
    }
}
impl Drop for Icon {
    fn drop(&mut self) {
        unsafe { sdl::SDL_DestroySurface(self.surf) };
    }
}

fn menu_check(menu: *mut sdl::SDL_TrayMenu, title: &'static CStr, checked: bool) -> *mut sdl::SDL_TrayEntry {
    let checked = if checked { sdl::SDL_TRAYENTRY_CHECKED } else { 0 };
    unsafe { sdl::SDL_InsertTrayEntryAt(menu, -1, title.as_ptr(), sdl::SDL_TRAYENTRY_CHECKBOX | checked) }
}

extern "C" fn c_menu_callback(userdata: *mut c_void, _entry: *mut sdl::SDL_TrayEntry) {
    #[allow(clippy::borrowed_box)] // needs to be boxed
    let f: &Box<dyn Fn()> = unsafe { &*(userdata as *mut Box<dyn Fn()>) };
    f();
}

fn menu_callback(entry: *mut sdl::SDL_TrayEntry, f: impl Fn()) {
    let f: Box<Box<dyn Fn()>> = Box::new(Box::new(f));
    let f = Box::leak(f) as *mut Box<dyn Fn()> as *mut c_void;
    unsafe { sdl::SDL_SetTrayEntryCallback(entry, Some(c_menu_callback), f) };
}

#[derive(Clone, Copy)]
enum MenuEvent {
    ToggleCheck,
    SetShiftMode(ConfigShiftMode),
    Quit,
}

fn bind_menu_event(entry: *mut sdl::SDL_TrayEntry, tx: &mpsc::Sender<MenuEvent>, event: MenuEvent) {
    let tx = tx.clone();
    menu_callback(entry, move || {
        let _ = tx.send(event);
    });
}

fn main() {
    // Initial loading
    let mut config = Config::load();

    // Create tray icon with menu
    unsafe { sdl::SDL_SetHint(sdl::SDL_HINT_VIDEO_ALLOW_SCREENSAVER, c"1".as_ptr()) };
    assert!(unsafe { sdl::SDL_Init(sdl::SDL_INIT_VIDEO) });
    let icon = Icon::load(include_bytes!("../assets/ggoled.png"));
    let icon_error = Icon::load(include_bytes!("../assets/ggoled_error.png"));
    let tray = unsafe { sdl::SDL_CreateTray(icon.surf, c"ggoled".as_ptr()) };
    assert!(!tray.is_null());
    let menu = unsafe { sdl::SDL_CreateTrayMenu(tray) };
    assert!(!menu.is_null());
    let (menu_tx, menu_rx) = mpsc::channel::<MenuEvent>();

    let tm_time_check = menu_check(menu, c"Show time", config.show_time);
    bind_menu_event(tm_time_check, &menu_tx, MenuEvent::ToggleCheck);
    let tm_media_check = menu_check(menu, c"Show playing media", config.show_media);
    bind_menu_event(tm_media_check, &menu_tx, MenuEvent::ToggleCheck);
    let tm_notif_check = menu_check(menu, c"Show connection notifications", config.show_notifications);
    bind_menu_event(tm_notif_check, &menu_tx, MenuEvent::ToggleCheck);
    let tm_idle_check = menu_check(menu, c"Screensaver when idle", config.idle_timeout);
    bind_menu_event(tm_idle_check, &menu_tx, MenuEvent::ToggleCheck);
    // TODO: implement idle check on linux
    #[cfg(target_os = "linux")]
    unsafe {
        config.idle_timeout = false;
        sdl::SDL_SetTrayEntryChecked(tm_idle_check, false);
        sdl::SDL_SetTrayEntryEnabled(tm_idle_check, false);
    }

    let tm_shift_submenu_entry =
        unsafe { sdl::SDL_InsertTrayEntryAt(menu, -1, c"OLED screen shift".as_ptr(), sdl::SDL_TRAYENTRY_SUBMENU) };
    let tm_shift_submenu = unsafe { sdl::SDL_CreateTraySubmenu(tm_shift_submenu_entry) };
    let tm_shift_off = menu_check(
        tm_shift_submenu,
        c"Off",
        matches!(config.oled_shift, ConfigShiftMode::Off),
    );
    bind_menu_event(tm_shift_off, &menu_tx, MenuEvent::SetShiftMode(ConfigShiftMode::Off));
    let tm_shift_simple = menu_check(
        tm_shift_submenu,
        c"Simple",
        matches!(config.oled_shift, ConfigShiftMode::Simple),
    );
    bind_menu_event(
        tm_shift_simple,
        &menu_tx,
        MenuEvent::SetShiftMode(ConfigShiftMode::Simple),
    );
    let tm_quit = unsafe { sdl::SDL_InsertTrayEntryAt(menu, -1, c"Quit".as_ptr(), sdl::SDL_TRAYENTRY_BUTTON) };
    bind_menu_event(tm_quit, &menu_tx, MenuEvent::Quit);

    // Load icons
    let icon_hs_connect =
        Arc::new(bitmap_from_memory(include_bytes!("../assets/headset_connected.png"), 0x80).unwrap());
    let icon_hs_disconnect =
        Arc::new(bitmap_from_memory(include_bytes!("../assets/headset_disconnected.png"), 0x80).unwrap());

    // State
    let mgr = MediaControl::new();
    let mut last_time = Local::now() - TimeDelta::seconds(1);
    let mut last_media: Option<Media> = None;
    let mut time_layers: Vec<LayerId> = vec![];
    let mut media_layers: Vec<LayerId> = vec![];
    let mut notif_layer: Option<LayerId> = None;
    let mut notif_expiry = Local::now();
    let mut is_connected = None; // TODO: probe on startup

    // Connect
    let mut dev = DrawDevice::new(dialog_unwrap(Device::connect()), 30);
    if let Some(font) = &config.font {
        dev.texter = dialog_unwrap(TextRenderer::load_from_file(&font.path, font.size));
    }

    // Go!
    dev.set_shift_mode(config.oled_shift.to_api());
    dev.play();
    'main: loop {
        // Window event loop
        // TODO: handle system going to sleep
        let mut event = sdl::SDL_Event::default();
        while unsafe { sdl::SDL_PollEvent(&mut event) } {
            let event_type = sdl::SDL_EventType(unsafe { event.r#type });
            if event_type == sdl::SDL_EVENT_QUIT {
                break 'main;
            }
        }

        // Handle tray menu events
        let mut config_updated = false;
        while let Ok(event) = menu_rx.try_recv() {
            match event {
                MenuEvent::ToggleCheck => {
                    config_updated = true;
                    config.show_time = unsafe { sdl::SDL_GetTrayEntryChecked(tm_time_check) };
                    config.show_media = unsafe { sdl::SDL_GetTrayEntryChecked(tm_media_check) };
                    config.show_notifications = unsafe { sdl::SDL_GetTrayEntryChecked(tm_notif_check) };
                    config.idle_timeout = unsafe { sdl::SDL_GetTrayEntryChecked(tm_idle_check) };
                }
                MenuEvent::SetShiftMode(mode) => {
                    config_updated = true;
                    config.oled_shift = mode;
                    unsafe { sdl::SDL_SetTrayEntryChecked(tm_shift_off, matches!(mode, ConfigShiftMode::Off)) };
                    unsafe { sdl::SDL_SetTrayEntryChecked(tm_shift_simple, matches!(mode, ConfigShiftMode::Simple)) };
                    dev.set_shift_mode(config.oled_shift.to_api());
                }
                MenuEvent::Quit => break 'main,
            }
        }
        if config_updated {
            dialog_unwrap(config.save());
        }

        let mut force_redraw = config_updated;

        // Handle events
        while let Some(event) = dev.try_event() {
            println!("event: {:?}", event);
            match event {
                DrawEvent::DeviceDisconnected => unsafe { sdl::SDL_SetTrayIcon(tray, icon_error.surf) },
                DrawEvent::DeviceReconnected => unsafe { sdl::SDL_SetTrayIcon(tray, icon.surf) },
                #[allow(clippy::single_match)]
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

                dev.play();
            }
        }

        sleep(Duration::from_millis(10));
    }
    let dev = dev.stop();
    dev.return_to_ui().unwrap();
}
