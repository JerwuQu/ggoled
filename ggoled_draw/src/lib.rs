// This is a wrapper around `ggoled_lib` that has high-level draw functions and additional events.
// Heavily specialised for `ggoled_cli` and `ggoled_app`, and is therefore not recommended for general use.

use anyhow::bail;
use ggoled_lib::{bitmap::BitVec, Bitmap, Device, DeviceEvent};
use image::{codecs::gif::GifDecoder, AnimationDecoder, ImageFormat, ImageReader};
use rusttype::{point, Font, Scale};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

pub struct TextRenderer {
    font: Font<'static>,
    size: f32,
}
impl TextRenderer {
    pub fn load_from_file(path: &PathBuf, size: f32) -> anyhow::Result<Self> {
        let data = std::fs::read(path)?;
        let Some(font) = Font::try_from_vec(data) else {
            bail!("Failed to load font");
        };
        Ok(Self { font, size })
    }
    pub fn new_pixel_operator() -> Self {
        Self {
            font: Font::try_from_bytes(include_bytes!("../fonts/PixelOperator.ttf")).unwrap(),
            size: 16.0,
        }
    }
    fn scale(&self) -> Scale {
        Scale::uniform(self.size)
    }
    pub fn line_height(&self) -> usize {
        let v_metrics = self.font.v_metrics(self.scale());
        (v_metrics.ascent - v_metrics.descent).ceil() as usize
    }
    pub fn render_lines(&self, text: &str) -> Vec<Bitmap> {
        let clean_text = text.replace('\r', "");
        let text_lines = clean_text.split('\n');
        text_lines
            .map(|text_line| {
                let glyphs: Vec<_> = self.font.layout(text_line, self.scale(), point(0.0, 0.0)).collect();
                let mut line_w_offset = 0;
                let mut line_h_offset = 0;
                let mut line_w = 0;
                let mut line_h = 0;
                for bb in glyphs.iter().filter_map(|g| g.pixel_bounding_box()) {
                    line_w_offset = line_w_offset.max(-bb.min.x);
                    line_h_offset = line_h_offset.max(-bb.min.y);
                    line_w = line_w.max(bb.max.x + 1);
                    line_h = line_h.max(bb.max.y + 1);
                }
                let line_w = (line_w + line_w_offset) as usize;
                let line_h = (line_h + line_h_offset) as usize;
                let mut bitmap = Bitmap::new(line_w, line_h, false);
                for glyph in glyphs {
                    if let Some(bb) = glyph.pixel_bounding_box() {
                        glyph.draw(|x, y, v| {
                            if v > 0.5 {
                                let px = (x as i32 + line_w_offset + bb.min.x) as usize;
                                let py = (y as i32 + line_h_offset + bb.min.y) as usize;
                                bitmap.data.set(py * line_w + px, true);
                            }
                        })
                    }
                }
                bitmap
            })
            .collect()
    }
}

fn bitmap_from_image(img: &image::RgbaImage, threshold: u8) -> Bitmap {
    Bitmap {
        w: img.width() as usize,
        h: img.height() as usize,
        data: img
            .pixels()
            .map(|p| (((p.0[0] as usize) + (p.0[1] as usize) + (p.0[2] as usize)) / 3) >= threshold as usize)
            .collect::<BitVec>(),
    }
}
fn bitmap_from_dynimage(img: &image::DynamicImage, threshold: u8) -> Bitmap {
    bitmap_from_image(&img.to_rgba8(), threshold)
}
pub fn bitmap_from_memory(buf: &[u8], threshold: u8) -> anyhow::Result<Bitmap> {
    let img = image::load_from_memory(buf)?;
    Ok(bitmap_from_dynimage(&img, threshold))
}

#[derive(Clone)]
pub struct Frame {
    pub bitmap: Arc<Bitmap>,
    pub delay: Option<Duration>,
}

pub fn decode_frames(path: &str, threshold: u8) -> Vec<Frame> {
    let reader = ImageReader::open(path).expect("Failed to open image");
    if matches!(reader.format().unwrap(), ImageFormat::Gif) {
        let gif = GifDecoder::new(reader.into_inner()).expect("Failed to decode gif");
        let frames = gif.into_frames();
        frames
            .map(|frame| {
                let frame = frame.expect("Failed to decode gif frame");
                let bitmap = Arc::new(bitmap_from_image(frame.buffer(), threshold));
                Frame {
                    bitmap,
                    delay: Some(Duration::from_millis(frame.delay().numer_denom_ms().0 as u64)),
                }
            })
            .collect()
    } else {
        let img = reader.decode().expect("Failed to decode image");
        let bitmap = Arc::new(bitmap_from_dynimage(&img, threshold));
        vec![Frame { bitmap, delay: None }]
    }
}

#[derive(Clone, Copy, PartialEq, PartialOrd, Hash, Eq, Ord)]
pub struct LayerId(usize);
impl LayerId {
    pub fn none() -> LayerId {
        LayerId(0)
    }
}

pub enum DrawLayer {
    Image {
        bitmap: Arc<Bitmap>,
        x: isize,
        y: isize,
    },
    Animation {
        frames: Vec<Frame>,
        x: isize,
        y: isize,
        follow_fps: bool,
    },
    Scroll {
        bitmap: Arc<Bitmap>,
        y: isize,
    },
}

pub enum ShiftMode {
    Off,
    Simple,
}

enum DrawCommand {
    Play,
    Pause,
    SetShiftMode(ShiftMode),
    Stop,
}

#[derive(Debug)]
pub enum DrawEvent {
    DeviceDisconnected,
    DeviceReconnected,
    DeviceEvent(DeviceEvent),
}

struct AnimState {
    ticks: usize,
    next_update: Instant,
}
struct ScrollState {
    x: isize,
}

struct DrawLayerState {
    layer: DrawLayer,
    anim: AnimState,
    scroll: ScrollState,
}

const OLED_SHIFT_PERIOD: Duration = Duration::from_secs(90);
const OLED_SHIFTS: [(isize, isize); 9] = [
    (0, 0),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

const RECONNECT_PERIOD: Duration = Duration::from_secs(1);

fn run_draw_device_thread(
    mut dev: Device,
    layers: Arc<Mutex<LayerMap>>,
    cmd_receiver: Receiver<DrawCommand>,
    event_sender: Sender<DrawEvent>,
    fps: usize,
) -> Device {
    let frame_delay = Duration::from_nanos(1_000_000_000 / fps as u64);
    let mut prev_screen = Bitmap::new(0, 0, false);
    let mut playing = false;
    let mut oled_shift = 0;
    let mut last_shift = Instant::now();
    let mut shift_mode = ShiftMode::Off;
    let mut connected = true;
    let mut last_connect_attempt = Instant::now();
    let mut last_frame_time = Instant::now();
    loop {
        let time = Instant::now();
        let mut stop_after_frame = false;
        while let Ok(cmd) = cmd_receiver.try_recv() {
            match cmd {
                DrawCommand::Play => playing = true,
                DrawCommand::Pause => playing = false,
                DrawCommand::SetShiftMode(mode) => shift_mode = mode,
                DrawCommand::Stop => stop_after_frame = true,
            }
        }

        // Attempt to reconnect
        if !connected && time.duration_since(last_connect_attempt) >= RECONNECT_PERIOD {
            last_connect_attempt = time;
            if dev.reconnect().is_ok() {
                connected = true;
                event_sender.send(DrawEvent::DeviceReconnected).unwrap();
            }
        }

        // Render frame
        if connected && playing {
            // Handle OLED shifts
            let (shift_x, shift_y) = match shift_mode {
                ShiftMode::Off => (0, 0),
                ShiftMode::Simple => {
                    if time.duration_since(last_shift) >= OLED_SHIFT_PERIOD {
                        oled_shift = (oled_shift + 1) % OLED_SHIFTS.len();
                        last_shift = time;
                    }
                    OLED_SHIFTS[oled_shift]
                }
            };

            // Update and blit each layer to the screen
            let mut screen = Bitmap::new(dev.width, dev.height, false);
            let mut layers = layers.lock().unwrap();
            for (_, state) in layers.iter_mut() {
                match &state.layer {
                    DrawLayer::Image { bitmap, x, y } => screen.blit(bitmap, x + shift_x, y + shift_y, false),
                    DrawLayer::Animation {
                        frames,
                        x,
                        y,
                        follow_fps,
                    } => {
                        if !frames.is_empty() {
                            let frame = &frames[state.anim.ticks % frames.len()];
                            screen.blit(&frame.bitmap, x + shift_x, y + shift_y, false);
                            if *follow_fps {
                                state.anim.ticks += 1;
                            } else if time >= state.anim.next_update {
                                state.anim.ticks += 1;
                                // TODO: handle 0 delay frames properly
                                // TODO: handle falling behind
                                if let Some(delay) = frame.delay {
                                    state.anim.next_update += delay;
                                }
                            }
                        }
                    }
                    DrawLayer::Scroll { bitmap, y } => {
                        const MARGIN: isize = 30;
                        let scroll_w = bitmap.w as isize + MARGIN;
                        let dupes = 1 + dev.width / scroll_w as usize;
                        for i in 0..=dupes {
                            screen.blit(
                                bitmap,
                                state.scroll.x + i as isize * scroll_w + shift_x,
                                *y + shift_y,
                                false,
                            );
                        }
                        state.scroll.x -= 1;
                        if state.scroll.x <= -scroll_w {
                            state.scroll.x += scroll_w;
                        }
                    }
                }
            }

            // Draw update
            let frame_time = Instant::now();
            let force_redraw = frame_time.duration_since(last_frame_time) >= Duration::from_secs(1);
            if screen != prev_screen || force_redraw {
                last_frame_time = frame_time;
                if let Err(_err) = dev.draw(&screen, 0, 0) {
                    if connected {
                        connected = false;
                        event_sender.send(DrawEvent::DeviceDisconnected).unwrap();
                    }
                } else {
                    prev_screen = screen;
                }
            }
            drop(layers);
        }

        // Get device events and pass back to DrawDevice
        if connected {
            let events = dev.get_events().unwrap_or_else(|_| {
                connected = false;
                event_sender.send(DrawEvent::DeviceDisconnected).unwrap();
                vec![]
            });
            for event in events {
                event_sender.send(DrawEvent::DeviceEvent(event)).unwrap();
            }
        }

        // Stop
        if stop_after_frame {
            break;
        }

        // Delay as long as needed based on how long frame rendering took (which will mostly depend on USB speed)
        let frame_duration = Instant::now().duration_since(time);
        // println!("frame: {:?}, {:?}", frame_duration, frame_delay);
        spin_sleep::sleep(frame_delay.saturating_sub(frame_duration));
    }
    dev
}

type LayerMap = BTreeMap<LayerId, DrawLayerState>;
pub struct DrawDevice {
    width: usize,
    height: usize,
    layers: Arc<Mutex<LayerMap>>,
    layer_counter: usize,
    thread: Option<std::thread::JoinHandle<Device>>,
    cmd_sender: Sender<DrawCommand>,
    event_receiver: Receiver<DrawEvent>,
    pub texter: TextRenderer,
}
impl DrawDevice {
    pub fn new(dev: Device, fps: usize) -> DrawDevice {
        let layers: Arc<Mutex<LayerMap>> = Default::default();
        let (cmd_sender, cmd_recver) = channel::<DrawCommand>();
        let (event_sender, event_receiver) = channel::<DrawEvent>();
        let c_layers = layers.clone();
        let (width, height) = (dev.width, dev.height);
        let thread = Some(std::thread::spawn(move || {
            run_draw_device_thread(dev, c_layers, cmd_recver, event_sender, fps)
        }));
        DrawDevice {
            width,
            height,
            layers,
            layer_counter: 0,
            thread,
            cmd_sender,
            event_receiver,
            texter: TextRenderer::new_pixel_operator(),
        }
    }
    fn destroy(&mut self) -> Option<Device> {
        if let Some(thread) = self.thread.take() {
            self.cmd_sender.send(DrawCommand::Stop).unwrap();
            Some(thread.join().unwrap())
        } else {
            None
        }
    }
    pub fn stop(mut self) -> Device {
        self.destroy().unwrap()
    }
    pub fn try_event(&mut self) -> Option<DrawEvent> {
        self.event_receiver.try_recv().ok()
    }
    pub fn poll_event(&mut self) -> DrawEvent {
        self.event_receiver.recv().unwrap()
    }
    pub fn center_bitmap(&self, bitmap: &Bitmap) -> (isize, isize) {
        (
            (self.width as isize - bitmap.w as isize) / 2,
            (self.height as isize - bitmap.h as isize) / 2,
        )
    }
    fn add_layer_locked(&mut self, layers: &mut MutexGuard<'_, LayerMap>, layer: DrawLayer) -> LayerId {
        self.layer_counter += 1;
        let id = LayerId(self.layer_counter);
        _ = layers.insert(
            id,
            DrawLayerState {
                layer,
                anim: AnimState {
                    ticks: 0,
                    next_update: Instant::now(),
                },
                scroll: ScrollState { x: 0 },
            },
        );
        id
    }
    pub fn add_layer(&mut self, layer: DrawLayer) -> LayerId {
        self.add_layer_locked(&mut self.layers.clone().lock().unwrap(), layer)
    }
    pub fn remove_layer(&mut self, id: LayerId) {
        self.layers.lock().unwrap().remove(&id);
    }
    pub fn remove_layers(&mut self, ids: &[LayerId]) {
        let mut layers = self.layers.lock().unwrap();
        for id in ids {
            layers.remove(id);
        }
    }
    pub fn clear_layers(&mut self) {
        self.layers.lock().unwrap().clear();
    }
    pub fn font_line_height(&self) -> usize {
        self.texter.line_height()
    }
    pub fn add_text(&mut self, text: &str, x: Option<isize>, y: Option<isize>) -> Vec<LayerId> {
        let layers = self.layers.clone();
        let mut layers = layers.lock().unwrap();
        let bitmaps: Vec<_> = self.texter.render_lines(text).into_iter().map(Arc::new).collect();
        let line_height = self.texter.line_height();
        let center_y: isize = (self.height as isize - (line_height * bitmaps.len()) as isize) / 2;
        bitmaps
            .into_iter()
            .enumerate()
            .map(|(i, bitmap)| {
                let y = y.unwrap_or(center_y) + (i * line_height) as isize;
                if bitmap.w >= self.width {
                    self.add_layer_locked(&mut layers, DrawLayer::Scroll { bitmap, y })
                } else {
                    let center = self.center_bitmap(&bitmap);
                    self.add_layer_locked(
                        &mut layers,
                        DrawLayer::Image {
                            bitmap,
                            x: x.unwrap_or(center.0),
                            y,
                        },
                    )
                }
            })
            .collect()
    }
    pub fn set_shift_mode(&mut self, mode: ShiftMode) {
        self.cmd_sender.send(DrawCommand::SetShiftMode(mode)).unwrap();
    }
    // TODO: atomic layer updates instead of play/pause (use `layers` handle with guard? renderer can use `try_lock` to avoid delaying frames)
    pub fn play(&mut self) {
        self.cmd_sender.send(DrawCommand::Play).unwrap();
    }
    pub fn pause(&mut self) {
        self.cmd_sender.send(DrawCommand::Pause).unwrap();
    }
}
impl Drop for DrawDevice {
    fn drop(&mut self) {
        self.destroy();
    }
}
