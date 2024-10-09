use clap::{command, Parser, ValueEnum};
use core::str;
use ggoled_draw::bitmap_from_memory;
use ggoled_draw::decode_frames;
use ggoled_draw::DrawDevice;
use ggoled_lib::Bitmap;
use ggoled_lib::Device;
use spin_sleep::sleep;
use std::{
    io::{stdin, Read},
    ops::Div,
    str::FromStr,
    time::{Duration, SystemTime},
};

#[derive(Clone, Copy)]
enum DrawPos {
    Coord(isize),
    Center,
}
impl FromStr for DrawPos {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<isize>().map(|n| Ok(DrawPos::Coord(n))).unwrap_or_else(|_| {
            if ["center", "c"].contains(&s.to_lowercase().as_str()) {
                Ok(DrawPos::Center)
            } else {
                Err("not a valid position")
            }
        })
    }
}
impl DrawPos {
    fn to_option(self) -> Option<isize> {
        match self {
            DrawPos::Coord(c) => Some(c),
            DrawPos::Center => None,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Alignment {
    #[value(alias("l"))]
    Left,
    #[value(alias("c"))]
    Center,
    #[value(alias("r"))]
    Right,
}

#[derive(Clone, Copy, ValueEnum)]
enum ScrollSpeed {
    Slow,
    Normal,
    Fast,
}

#[derive(clap::Args)]
struct DrawArgs {
    #[arg(
        short = 'x',
        long,
        help = "Screen X offset for draw commands",
        default_value = "center"
    )]
    screen_x: DrawPos,

    #[arg(
        short = 'y',
        long,
        help = "Screen Y offset for draw commands",
        default_value = "center"
    )]
    screen_y: DrawPos,
}

#[derive(clap::Args)]
struct ImageArgs {
    #[command(flatten)]
    draw_args: DrawArgs,

    #[arg(
        short = 'T',
        long,
        help = "Grayscale threshold for converting images to 1-bit",
        default_value = "100"
    )]
    threshold: u8,
}

#[derive(Parser)]
enum Args {
    #[command(about = "Clear the entire screen to black")]
    Clear,

    #[command(about = "Fill the entire screen to white")]
    Fill,

    #[command(about = "Draw some text")]
    Text {
        #[command(flatten)]
        draw_args: DrawArgs,

        #[arg(help = "Text, or omitted for stdin", index = 1)]
        text: Option<String>,

        #[arg(short = 'd', long, help = "Screen delimiter line for stdin input")]
        delimiter: Option<String>,
    },

    #[command(about = "Draw an image")]
    Img {
        #[command(flatten)]
        image_args: ImageArgs,

        #[arg(help = "Image path, or - for stdin", index = 1)]
        path: String,
    },

    #[command(about = "Draw a sequence of images")]
    Anim {
        #[command(flatten)]
        image_args: ImageArgs,

        #[arg(
            short = 'r',
            long,
            help = "Frames to show per second (fps) - defaults to 1 fps or embedded delays for gif files"
        )]
        framerate: Option<u32>,

        #[arg(
            short = 'l',
            long,
            help = "Amount of repetitions, or 0 for infinite",
            default_value = "1"
        )]
        loops: usize,

        #[arg(help = "Image paths", index = 1)]
        paths: Vec<String>,
    },
}

fn main() {
    let args = Args::parse();
    let dev = Device::connect().unwrap();

    match args {
        Args::Clear => dev.draw(&Bitmap::new(dev.width, dev.height, false), 0, 0).unwrap(),
        Args::Fill => dev.draw(&Bitmap::new(dev.width, dev.height, true), 0, 0).unwrap(),
        Args::Text {
            text,
            draw_args,
            delimiter,
        } => {
            let mut dev = DrawDevice::new(dev, 30);
            if let Some(text) = text {
                dev.add_text(&text, draw_args.screen_x.to_option(), draw_args.screen_y.to_option());
                dev.play();
            } else {
                dev.play();
                // iterate each line in stdin and draw to screen either when reaching EOF or when encountering `delimiter`
                let mut lines = vec![];
                for line in stdin().lines() {
                    let line = line.expect("Failed to read from stdin").replace('\r', "");
                    if Some(&line) == delimiter.as_ref() {
                        dev.clear_layers();
                        dev.add_text(
                            &lines.join("\n"),
                            draw_args.screen_x.to_option(),
                            draw_args.screen_y.to_option(),
                        );
                        lines.clear();
                    } else {
                        lines.push(line);
                    }
                }
                if !lines.is_empty() {
                    dev.clear_layers();
                    dev.add_text(
                        &lines.join("\n"),
                        draw_args.screen_x.to_option(),
                        draw_args.screen_y.to_option(),
                    );
                }
            }
            dev.await_frame();
        }
        Args::Img { path, image_args } => {
            let bitmap = if path == "-" {
                let mut buf = Vec::<u8>::new();
                stdin().read_to_end(&mut buf).expect("Failed to read from stdin");
                bitmap_from_memory(&buf, image_args.threshold).expect("Failed to read image from stdin")
            } else {
                let mut frames = decode_frames(&path, image_args.threshold);
                if frames.len() != 1 {
                    eprintln!("img only supports images with single frame");
                }
                frames.swap_remove(0).bitmap
            };
            dev.draw(&bitmap, 0, 0).unwrap();
        }
        Args::Anim {
            framerate,
            loops,
            paths,
            image_args,
        } => {
            if framerate == Some(0) {
                panic!("Framerate must be non-zero");
            } else if paths.is_empty() {
                panic!("No image paths");
            }
            let period = framerate.map(|f| Duration::from_secs(1).div(f));
            let bitmaps: Vec<(Bitmap, Duration)> = paths
                .iter()
                .flat_map(|path| {
                    decode_frames(path, image_args.threshold).into_iter().map(|frame| {
                        (
                            frame.bitmap,
                            period.unwrap_or(frame.delay.unwrap_or(Duration::from_secs(1))),
                        )
                    })
                })
                .collect();
            let mut frame_idx = 0;
            let mut draw_animation = || {
                for (bitmap, delay) in &bitmaps {
                    let now_time = SystemTime::now();
                    let next_frame = now_time + *delay;
                    let cx = (dev.width as isize - bitmap.w as isize) / 2;
                    let cy = (dev.width as isize - bitmap.w as isize) / 2;
                    let x = image_args.draw_args.screen_x.to_option().unwrap_or(cx);
                    let y = image_args.draw_args.screen_y.to_option().unwrap_or(cy);
                    dev.draw(bitmap, x, y).unwrap();
                    frame_idx += 1;
                    if now_time < next_frame {
                        sleep(next_frame.duration_since(SystemTime::now()).unwrap());
                    } else {
                        println!("fell behind - framerate too fast");
                    }
                }
            };
            if loops == 0 {
                loop {
                    draw_animation();
                }
            } else {
                for _ in 0..loops {
                    draw_animation();
                }
            }
        }
    }
}
