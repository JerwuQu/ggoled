#![windows_subsystem = "windows"]

use chrono::{Local, TimeDelta, Timelike};
use ggoled_draw::{DrawDevice, LayerId};
use ggoled_lib::Device;
use media::{Media, MediaControl};
use std::{ptr::null_mut, thread::sleep, time::Duration};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIconBuilder,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG};
mod media;

fn main() {
    let icon_png = include_bytes!("../ggoled.png");
    let icon_rgba = image::load_from_memory(icon_png)
        .unwrap()
        .resize(32, 32, image::imageops::FilterType::Lanczos3)
        .to_rgba8();

    let tray_menu = Menu::new();
    let quit = MenuItem::new("Quit", true, None);
    tray_menu.append_items(&[&quit]).unwrap();
    let icon = Icon::from_rgba(icon_rgba.to_vec(), icon_rgba.width(), icon_rgba.height()).unwrap();
    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("ggoled")
        .with_icon(icon)
        .build()
        .unwrap();

    println!("Running");

    let mut dev = DrawDevice::new(Device::connect().unwrap(), 30);
    dev.play();

    let mgr = MediaControl::new();

    let menu_channel = MenuEvent::receiver();
    let mut last_time = Local::now() - TimeDelta::seconds(1);
    let mut last_media: Option<Media> = None;
    let mut time_layers: Vec<LayerId> = vec![];
    let mut media_layers: Vec<LayerId> = vec![];
    // TODO: screensaver when computer is unused or locked
    loop {
        // Window event loop is required to get tray-icon working
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, 1) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Update time every second
        let time = Local::now();
        if time.second() != last_time.second() {
            let time_str = time.format("%H:%M:%S").to_string();
            let media = mgr.get_media(); // also only fetch media once a second

            dev.pause();
            dev.remove_layers(&time_layers);
            time_layers = dev.add_text(&time_str, None, Some(8));
            last_time = time;

            if media != last_media {
                dev.remove_layers(&media_layers);
                if let Some(m) = &media {
                    media_layers = dev.add_text(
                        &format!("{}\n{}", m.title, m.artist),
                        None,
                        Some(8 + dev.font_line_height() as isize),
                    );
                } else {
                    media_layers = vec![]
                }
                last_media = media;
            }
            dev.play();
        }

        if let Ok(event) = menu_channel.try_recv() {
            if event.id == quit.id() {
                dev.clear_layers();
                dev.await_frame();
                return;
            }
        }

        sleep(Duration::from_millis(10));
    }
}
