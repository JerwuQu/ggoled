use std::{
    collections::BTreeMap,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, SystemTime},
};

use ggoled_lib::{bitmap::BitVec, Bitmap, Device};
use image::{codecs::gif::GifDecoder, io::Reader as ImageReader, AnimationDecoder, ImageFormat};
use rusttype::{point, Font, Scale};

pub struct TextRenderer {
    font: Font<'static>,
}
impl TextRenderer {
    pub fn create() -> Self {
        let font = Font::try_from_bytes(include_bytes!("../fonts/PixelOperator.ttf")).unwrap();
        Self { font }
    }
    fn scale() -> Scale {
        Scale::uniform(16.0)
    }
    pub fn line_height(&self) -> usize {
        let v_metrics = self.font.v_metrics(Self::scale());
        (v_metrics.ascent - v_metrics.descent).ceil() as usize
    }
    pub fn render_lines(&self, text: &str) -> Vec<Bitmap> {
        let clean_text = text.replace('\r', "");
        let text_lines = clean_text.split('\n');
        text_lines
            .map(|text_line| {
                let glyphs: Vec<_> = self.font.layout(text_line, Self::scale(), point(0.0, 0.0)).collect();
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

pub struct Frame {
    pub bitmap: Bitmap,
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
                let bitmap = bitmap_from_image(frame.buffer(), threshold);
                Frame {
                    bitmap,
                    delay: Some(Duration::from_millis(frame.delay().numer_denom_ms().0 as u64)),
                }
            })
            .collect()
    } else {
        let img = reader.decode().expect("Failed to decode image");
        let bitmap = bitmap_from_dynimage(&img, threshold);
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
pub struct Pos {
    pub x: isize,
    pub y: isize,
}

pub enum DrawLayer {
    Image {
        bitmap: Bitmap,
        pos: Pos,
    },
    Animation {
        frames: Vec<Frame>,
        pos: Pos,
        follow_fps: bool,
    },
    Scroll {
        bitmap: Bitmap,
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
    AwaitFrame,
    SetShiftMode(ShiftMode),
    Stop,
}

struct AnimState {
    ticks: usize,
    next_update: SystemTime,
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

fn run_draw_device_thread(
    dev: Device,
    layers: Arc<Mutex<LayerMap>>,
    cmd_receiver: Receiver<DrawCommand>,
    frame_sender: Sender<()>,
    fps: usize,
) {
    let frame_delay = Duration::from_nanos(1_000_000_000 / fps as u64);
    let mut prev_screen = Bitmap::new(0, 0, false);
    let mut signal_update = false;
    let mut playing = false;
    let mut oled_shift = 0;
    let mut last_shift = SystemTime::now();
    let mut shift_mode = ShiftMode::Off;
    loop {
        let time = SystemTime::now();
        while let Ok(cmd) = cmd_receiver.try_recv() {
            match cmd {
                DrawCommand::Play => playing = true,
                DrawCommand::Pause => playing = false,
                DrawCommand::AwaitFrame => signal_update = true,
                DrawCommand::SetShiftMode(mode) => shift_mode = mode,
                DrawCommand::Stop => return,
            }
        }
        if playing {
            let (shift_x, shift_y) = match shift_mode {
                ShiftMode::Off => (0, 0),
                ShiftMode::Simple => {
                    if time.duration_since(last_shift).unwrap() >= OLED_SHIFT_PERIOD {
                        oled_shift = (oled_shift + 1) % OLED_SHIFTS.len();
                        last_shift = time;
                    }
                    OLED_SHIFTS[oled_shift]
                }
            };

            let mut screen = Bitmap::new(dev.width, dev.height, false);
            let mut layers = layers.lock().unwrap();
            for (_, state) in layers.iter_mut() {
                match &state.layer {
                    DrawLayer::Image { bitmap, pos } => screen.blit(bitmap, pos.x + shift_x, pos.y + shift_y, false),
                    DrawLayer::Animation {
                        frames,
                        pos,
                        follow_fps,
                    } => {
                        if !frames.is_empty() {
                            let frame = &frames[state.anim.ticks % frames.len()];
                            screen.blit(&frame.bitmap, pos.x + shift_x, pos.y + shift_y, false);
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
            if screen != prev_screen {
                dev.draw(&screen, 0, 0).unwrap();
                prev_screen = screen;
            }
            if signal_update {
                signal_update = false;
                frame_sender.send(()).unwrap();
            }
            drop(layers);
        }
        spin_sleep::sleep(frame_delay); // TODO: calculate how long to actually sleep for
    }
}

type LayerMap = BTreeMap<LayerId, DrawLayerState>;
pub struct DrawDevice {
    width: usize,
    height: usize,
    layers: Arc<Mutex<LayerMap>>,
    layer_counter: usize,
    thread: Option<std::thread::JoinHandle<()>>,
    cmd_sender: Sender<DrawCommand>,
    frame_recver: Receiver<()>,
    texter: TextRenderer,
}
impl DrawDevice {
    pub fn new(dev: Device, fps: usize) -> DrawDevice {
        let layers: Arc<Mutex<LayerMap>> = Default::default();
        let (cmd_sender, cmd_recver) = channel::<DrawCommand>();
        let (frame_sender, frame_recver) = channel::<()>();
        let c_layers = layers.clone();
        let (width, height) = (dev.width, dev.height);
        let thread = Some(std::thread::spawn(move || {
            run_draw_device_thread(dev, c_layers, cmd_recver, frame_sender, fps)
        }));
        DrawDevice {
            width,
            height,
            layers,
            layer_counter: 0,
            thread,
            cmd_sender,
            frame_recver,
            texter: TextRenderer::create(),
        }
    }
    pub fn center_bitmap(&self, bitmap: &Bitmap) -> Pos {
        Pos {
            x: (self.width as isize - bitmap.w as isize) / 2,
            y: (self.height as isize - bitmap.h as isize) / 2,
        }
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
                    next_update: SystemTime::now(),
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
        let bitmaps = self.texter.render_lines(text);
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
                            pos: Pos {
                                x: x.unwrap_or(center.x),
                                y,
                            },
                        },
                    )
                }
            })
            .collect()
    }
    pub fn await_frame(&mut self) {
        self.cmd_sender.send(DrawCommand::AwaitFrame).unwrap();
        self.frame_recver.recv().unwrap();
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
        self.cmd_sender.send(DrawCommand::Stop).unwrap();
        self.thread.take().unwrap().join().unwrap();
    }
}
