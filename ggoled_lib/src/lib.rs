pub mod bitmap;
use anyhow::bail;
pub use bitmap::Bitmap;
use hidapi::{HidApi, HidDevice, MAX_REPORT_DESCRIPTOR_SIZE};
use std::cmp::min;

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
    Volume { volume: u8 },
    Battery { headset: u8, charging: u8 },
    HeadsetConnection { connected: bool },
}

pub struct Device {
    oled_dev: HidDevice,
    info_dev: HidDevice,
    pub width: usize,
    pub height: usize,
}
impl Device {
    /// Connect to a SteelSeries GG device.
    pub fn connect() -> anyhow::Result<Device> {
        let api = HidApi::new().unwrap();

        // Find all connected devices matching given Vendor/Product IDs and interface
        let device_infos: Vec<_> = api
            .device_list()
            .filter(|d| {
                d.vendor_id() == 0x1038 // SteelSeries
        && [
            0x12cb, // Arctis Nova Pro Wired
            0x12e0, // Arctis Nova Pro Wireless
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

        // Open all such devices
        let Ok(mut devices) = device_infos
            .iter()
            .map(|info| anyhow::Ok(info.open_device(&api)?))
            .collect::<anyhow::Result<Vec<_>>>()
        else {
            bail!("Failed to connect to USB device");
        };

        // Get HID reports for all devices
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

        // Grab the two devices by their descriptors
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

        Ok(Device {
            oled_dev,
            info_dev,
            width: 128,
            height: 64,
        })
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
        report[4] = d.w as u8;
        report[5] = d.h as u8;
        let stride_h = (d.dst_y.wrapping_rem(8) + d.h).div_ceil(8) * 8; // TODO: fuzz this with all x/y/w/h combinations
        for y in 0..d.h {
            for x in 0..d.w {
                // NOTE: report has columns rather than rows
                let ri = x * stride_h + y;
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
            w -= (-x) as usize;
            (0, (-x) as usize)
        } else {
            (x as usize, 0)
        };
        let (y, src_y) = if y < 0 {
            h -= (-y) as usize;
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
            self.oled_dev.send_feature_report(&report)?;
        }
        Ok(())
    }

    /// Set screen brightness.
    pub fn set_brightness(&self, value: u8) -> anyhow::Result<()> {
        if value < 0x01 {
            bail!("brightness too low!");
        } else if value > 0x0a {
            bail!("brightness too high!");
        }
        let mut report = [0; 64];
        report[0] = 0x06; // hid report id
        report[1] = 0x85; // command id
        report[2] = value;
        self.oled_dev.write(&report)?;
        Ok(())
    }

    fn parse_event(buf: &[u8; 64]) -> Option<DeviceEvent> {
        #[cfg(debug_assertions)]
        println!("parse_event: {:x?}", buf);
        if buf[0] != 7 {
            return None;
        }
        Some(match buf[1] {
            0x25 => DeviceEvent::Volume {
                volume: 0x38u8.saturating_sub(buf[2]),
            },
            0xb5 => DeviceEvent::HeadsetConnection { connected: buf[4] == 8 },
            0xb7 => DeviceEvent::Battery {
                headset: buf[2],
                charging: buf[3],
                // NOTE: there's a chance `buf[4]` represents the max value, but i don't have any other devices to test with
            },
            _ => return None,
        })
    }

    /// Poll events from the device. This blocks until an event is returned.
    pub fn poll_event(&self) -> anyhow::Result<Option<DeviceEvent>> {
        let mut buf = [0u8; 64];
        self.info_dev.set_blocking_mode(true)?;
        _ = self.info_dev.read(&mut buf)?;
        Ok(Self::parse_event(&buf))
    }

    /// Return any pending events from the device. Non-blocking.
    pub fn get_events(&self) -> anyhow::Result<Vec<DeviceEvent>> {
        self.info_dev.set_blocking_mode(false)?;
        let mut events = vec![];
        loop {
            let mut buf = [0u8; 64];
            let len = self.info_dev.read(&mut buf)?;
            if len == 0 {
                break;
            } else if let Some(event) = Self::parse_event(&buf) {
                events.push(event);
            }
        }
        Ok(events)
    }
}
