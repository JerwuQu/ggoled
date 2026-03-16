pub mod bitmap;
use anyhow::bail;
pub use bitmap::Bitmap;
use hidapi::{HidApi, HidDevice, MAX_REPORT_DESCRIPTOR_SIZE};
use std::{cmp::min, time::Duration};

// NOTE: these work for Arctis Nova Pro but might not for different products!
const SCREEN_REPORT_SPLIT_SZ: usize = 64;
const SCREEN_REPORT_SIZE: usize = 1024;

type DrawReport = [u8; SCREEN_REPORT_SIZE];

struct ReportDrawable<'a> {
    bitmap: &'a Bitmap,
    w: usize,
    h: usize,
    dst_x: usize,
    dst_y: usize,
    src_x: usize,
    src_y: usize,
}

#[derive(Debug)]
pub enum DeviceEvent {
    Volume {
        volume: u8,
    },
    Battery {
        headset: u8,
        charging: u8,
    },
    HeadsetConnection {
        wireless: bool,
        bluetooth: bool,
        bluetooth_on: bool,
    },
}

enum DeviceMerge {
    Merged(HidDevice),
    Separate { oled: HidDevice, info: HidDevice },
}
impl DeviceMerge {
    fn oled(&self) -> &HidDevice {
        match self {
            DeviceMerge::Merged(dev) => dev,
            DeviceMerge::Separate { oled, .. } => oled,
        }
    }
    fn info(&self) -> &HidDevice {
        match self {
            DeviceMerge::Merged(dev) => dev,
            DeviceMerge::Separate { info, .. } => info,
        }
    }
}

pub struct Device {
    dev: DeviceMerge,
    pub width: usize,
    pub height: usize,
}
impl Device {
    /// Connect to a SteelSeries GG device.
    pub fn connect() -> anyhow::Result<Device> {
        let api = HidApi::new()?;

        // Find all connected devices matching given Vendor/Product IDs and interface
        let device_infos: Vec<_> = api
            .device_list()
            .filter(|d| {
                d.vendor_id() == 0x1038 // SteelSeries
        && [
            0x12cb, // Arctis Nova Pro Wired
            0x12cd, // Arctis Nova Pro Wired (Xbox)
            0x12e0, // Arctis Nova Pro Wireless
            0x12e5, // Arctis Nova Pro Wireless (Xbox)
            0x225d, // Arctis Nova Pro Wireless (Xbox White)
        ].contains(&d.product_id()) && d.interface_number() == 4
            })
            .collect();

        // We're expecting to find exactly two devices with different HID descriptors
        if device_infos.is_empty() {
            bail!("No matching devices connected");
        } else if device_infos.len() < 2 {
            bail!("Too few matching devices connected");
        } else if device_infos.len() > 2 {
            bail!("Too many matching devices connected");
        }

        // On Linux, both devices can get put under the same hidraw interface, meaning we use the same device for both
        let dev = if device_infos[0].path() == device_infos[1].path() {
            let Ok(dev) = device_infos[0].open_device(&api) else {
                bail!("Failed to connect to USB device");
            };
            DeviceMerge::Merged(dev)
        // On Windows (and maybe some Linux variants), they are separate interfaces and have to be opened separately
        } else {
            // Open both devices
            let Ok(mut devices) = device_infos
                .iter()
                .map(|info| anyhow::Ok(info.open_device(&api)?))
                .collect::<anyhow::Result<Vec<_>>>()
            else {
                bail!("Failed to connect to USB device");
            };

            // Get descriptors
            let Ok(mut device_reports) = devices
                .iter()
                .map(|dev| {
                    let mut buf = [0u8; MAX_REPORT_DESCRIPTOR_SIZE];
                    let sz = dev.get_report_descriptor(&mut buf)?;
                    anyhow::Ok(Vec::from(&buf[..sz]))
                })
                .collect::<anyhow::Result<Vec<_>>>()
            else {
                bail!("Failed to get USB device HID reports");
            };

            // Identify and open the two devices by their descriptors
            let Some(oled_dev_idx) = device_reports.iter().position(|desc| desc[1] == 0xc0) else {
                bail!("No OLED device found");
            };
            _ = device_reports.swap_remove(oled_dev_idx);
            let oled_dev = devices.swap_remove(oled_dev_idx);
            let Some(info_dev_idx) = device_reports.iter().position(|desc| desc[1] == 0x00) else {
                bail!("No info device found");
            };
            _ = device_reports.swap_remove(info_dev_idx);
            let info_dev = devices.swap_remove(info_dev_idx);

            DeviceMerge::Separate {
                oled: oled_dev,
                info: info_dev,
            }
        };

        Ok(Device {
            dev,
            width: 128,
            height: 64,
        })
    }

    /// Dump the full device tree info for all SteelSeries devices to stdout for debug purposes
    pub fn dump_devices() {
        let Ok(api) = HidApi::new() else {
            eprintln!("Failed to initialize HID API.");
            return;
        };

        let device_infos: Vec<_> = api
            .device_list()
            .filter(|d| d.vendor_id() == 0x1038) // SteelSeries
            .collect();
        if device_infos.is_empty() {
            println!("No devices.");
            return;
        }

        println!("-----");
        for info in device_infos {
            println!("product={}", info.product_string().unwrap_or("?"));
            println!("pid={:#04x}", info.product_id());
            println!("interface={}", info.interface_number());
            println!("path={}", info.path().to_string_lossy());
            println!("usage={}", info.usage());
            if let Ok(dev) = info.open_device(&api) {
                let mut buf = [0u8; MAX_REPORT_DESCRIPTOR_SIZE];
                if let Ok(sz) = dev.get_report_descriptor(&mut buf) {
                    println!("report desc sz={sz}, first 16 bytes: {:02x?}", &buf[0..16]);
                } else {
                    println!("getting report descriptor failed");
                }
            } else {
                println!("opening device failed");
            }
            println!("-----");
        }
    }

    /// Reconnect to a device.
    pub fn reconnect(&mut self) -> anyhow::Result<()> {
        *self = Self::connect()?;
        Ok(())
    }

    // Creates a HID report for a `ReportDrawable`
    // The Bitmap must already be within the report limits (from `split_for_report`)
    fn create_report(&self, d: &ReportDrawable) -> DrawReport {
        let mut report: DrawReport = [0; SCREEN_REPORT_SIZE];
        report[0] = 0x06; // hid report id
        report[1] = 0x93; // command id
        report[2] = d.dst_x as u8;
        report[3] = d.dst_y as u8;
        // Pad height to multiple of 8 to align with device blocks.
        let padded_h = d.h.div_ceil(8) * 8;
        report[4] = d.w as u8;
        report[5] = padded_h as u8;
        for y in 0..d.h {
            for x in 0..d.w {
                // NOTE: report has columns rather than rows
                let ri = x * padded_h + y;
                let pi = (d.src_y + y) * d.bitmap.w + (d.src_x + x);
                report[(ri / 8) + 6] |= (d.bitmap.data[pi] as u8) << (ri % 8);
            }
        }
        report
    }

    // Splits up a `Bitmap` to be appropriately sized for being able to send over USB HID
    fn prepare_for_report<'a>(&self, bitmap: &'a Bitmap, x: isize, y: isize) -> Vec<ReportDrawable<'a>> {
        let mut w = bitmap.w;
        let mut h = bitmap.h;

        // Handle negative x/y by moving src_x/src_y
        let (x, src_x) = if x < 0 {
            w = w.saturating_sub((-x) as usize);
            (0, (-x) as usize)
        } else {
            (x as usize, 0)
        };
        let (y, src_y) = if y < 0 {
            h = h.saturating_sub((-y) as usize);
            (0, (-y) as usize)
        } else {
            (y as usize, 0)
        };

        // Crop size to screen
        let x = min(x, self.width);
        let y = min(y, self.height);
        if x + w >= self.width {
            w = self.width - x;
        }
        if y + h >= self.height {
            h = self.height - y;
        }

        // Split
        let mut vec = Vec::<ReportDrawable<'a>>::new();
        let splits = w.div_ceil(SCREEN_REPORT_SPLIT_SZ);
        for i in 0..splits {
            vec.push(ReportDrawable {
                bitmap,
                w: min(SCREEN_REPORT_SPLIT_SZ, w - i * SCREEN_REPORT_SPLIT_SZ),
                h,
                dst_x: x + (i * SCREEN_REPORT_SPLIT_SZ),
                dst_y: y,
                src_x: src_x + i * SCREEN_REPORT_SPLIT_SZ,
                src_y,
            });
        }
        vec
    }

    /// Draw a `Bitmap` at the given location.
    pub fn draw(&self, bitmap: &Bitmap, x: isize, y: isize) -> anyhow::Result<()> {
        let drawables = self.prepare_for_report(bitmap, x, y);
        for drawable in drawables {
            let report = self.create_report(&drawable);
            self.retry_report(&report)?;
        }
        Ok(())
    }

    fn retry_report(&self, data: &[u8]) -> anyhow::Result<()> {
        let mut i: u64 = 0;
        loop {
            match self.dev.oled().send_feature_report(data) {
                Ok(_) => return Ok(()),
                Err(err) => {
                    if i == 10 {
                        return Err(err.into());
                    }
                    i += 1;
                    spin_sleep::sleep(Duration::from_millis(i.pow(2)));
                }
            }
        }
    }

    /// Set screen brightness.
    pub fn set_brightness(&self, value: u8) -> anyhow::Result<()> {
        if value < 1 {
            bail!("brightness too low");
        } else if value > 0x0a {
            bail!("brightness too high");
        }
        let mut report = [0; 64];
        report[0] = 0x06; // hid report id
        report[1] = 0x85; // command id
        report[2] = value;
        self.dev.oled().write(&report)?;
        Ok(())
    }

    /// Probe device to fetch current state.
    /// Data is received via events.
    pub fn probe(&self) -> anyhow::Result<()> {
        let mut report = [0; 64];
        report[0] = 0x06; // hid report id
        report[1] = 0xb0; // command id, get various data
        self.dev.info().write(&report)?;
        report[1] = 0x20; // command id, get volume info
        self.dev.info().write(&report)?;
        Ok(())
    }

    /// Return to SteelSeries UI.
    pub fn return_to_ui(&self) -> anyhow::Result<()> {
        let mut report = [0; 64];
        report[0] = 0x06; // hid report id
        report[1] = 0x95; // command id
        self.dev.oled().write(&report)?;
        Ok(())
    }

    fn parse_event(buf: &[u8; 64]) -> Vec<DeviceEvent> {
        #[cfg(debug_assertions)]
        println!("parse_event: {:x?}", buf);
        match (buf[0], buf[1]) {
            // --- events ---

            // only contains new volume
            (0x07, 0x25) => vec![DeviceEvent::Volume {
                volume: 0x38u8.saturating_sub(buf[2]),
            }],

            // weird bytes values, but seem consistent
            (0x07, 0xb5) => vec![DeviceEvent::HeadsetConnection {
                wireless: buf[4] == 8,
                bluetooth: buf[3] == 1,
                bluetooth_on: buf[2] == 4,
            }],

            // 0-8 values for battery levels
            // we handle both event and command reply the same (because they look the same)
            // can fetch this info with a [0x06, 0xb7] command, but [0x06, 0xb0] seems superior (?)
            (0x07, 0xb7) | (0x06, 0xb7) => vec![DeviceEvent::Battery {
                headset: buf[2],
                charging: buf[3],
                // NOTE: there's a possibility `buf[4]` represents either the max value or simply just `8` for connected
            }],

            // --- command responses ---

            // version info, fetch with [0x06, 0x10]
            // basically useless for us so not implemented
            (0x06, 0x10) => vec![],

            // [0x06, 0x20] returns a bunch of info
            // same regardless of connected state
            // i think some of it is equalizer levels, but i've got no idea what the rest is
            (0x06, 0x20) => vec![DeviceEvent::Volume {
                volume: 0x38u8.saturating_sub(buf[3]), // NOTE: different byte from Volume event
            }],

            // unknown data, fetch with [0x06, 0x80]
            // same regardless of connected state
            // not implemented
            (0x06, 0x80) => vec![],

            // various data
            // there's a couple of bytes i've got no idea what they're supposed to represent
            (0x06, 0xb0) => vec![
                DeviceEvent::HeadsetConnection {
                    wireless: buf[15] == 8,
                    bluetooth: buf[5] == 1,
                    bluetooth_on: buf[4] == 4,
                },
                DeviceEvent::Battery {
                    headset: buf[6],
                    charging: buf[7],
                    // NOTE: `buf[15]` seems to behave the same as `buf[4]` in the event
                },
            ],

            _ => vec![],
        }
    }

    /// Poll events from the device. This blocks until an event is returned.
    pub fn poll_event(&self) -> anyhow::Result<Vec<DeviceEvent>> {
        let mut buf = [0u8; 64];
        self.dev.info().set_blocking_mode(true)?;
        _ = self.dev.info().read(&mut buf)?;
        Ok(Self::parse_event(&buf))
    }

    /// Return any pending events from the device. Non-blocking.
    pub fn get_events(&self) -> anyhow::Result<Vec<DeviceEvent>> {
        self.dev.info().set_blocking_mode(false)?;
        let mut events = vec![];
        loop {
            let mut buf = [0u8; 64];
            let len = self.dev.info().read(&mut buf)?;
            if len == 0 {
                break;
            } else {
                events.append(&mut Self::parse_event(&buf));
            }
        }
        Ok(events)
    }
}
