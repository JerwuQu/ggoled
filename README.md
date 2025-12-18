# ggoled

Put custom graphics on your SteelSeries Arctis Nova Pro Base Station 128x64 OLED screen.

This utility implements the USB protocol, so you don't need SteelSeries GG/Engine Apps/GameSense, and it works on linux.

There is also a [desktop application](#desktop-application) available for Windows and Linux that shows the current time and currently playing media, along with some other features.

## Animation showcase

Bad Apple at 60 fps.
This also showcases the burn-in you will get if not careful with OLEDs. The flickering is due to bad camera settings and not actually shown on the display.

[![Bad Apple on the Base Station](http://img.youtube.com/vi/k51zNrMLti4/0.jpg)](http://www.youtube.com/watch?v=k51zNrMLti4 "Bad Apple on a SteelSeries Arctis Nova Pro Wireless Base Station")

## Supported Devices

| Device                                      | Supported                                             |
| ------------------------------------------- | ----------------------------------------------------- |
| SteelSeries Arctis Nova Pro Wired           | ✅                                                    |
| SteelSeries Arctis Nova Pro Wired (Xbox)    | ✅                                                    |
| SteelSeries Arctis Nova Pro Wireless        | ✅                                                    |
| SteelSeries Arctis Nova Pro Wireless (Xbox) | ✅                                                    |
| SteelSeries Arctis Pro Wired                | 🧐 [#12](https://github.com/JerwuQu/ggoled/issues/12) |
| SteelSeries Arctis Pro Wireless             | 🧐 [#12](https://github.com/JerwuQu/ggoled/issues/12) |
| SteelSeries Arctis Nova Elite               | 🧐 [#26](https://github.com/JerwuQu/ggoled/issues/26) |

PRs and issues for similar devices are welcome!

## Install

For Windows you can download the latest builds either from [GitHub Actions](https://github.com/JerwuQu/ggoled/actions?query=branch%3Amaster) or from [nightly.link (direct download)](https://nightly.link/JerwuQu/ggoled/workflows/build/master/x86_64-pc-windows-gnu.zip).

Otherwise, install the Rust toolchain and run: `cargo install --locked --git https://github.com/JerwuQu/ggoled.git ggoled ggoled_app`

To run `ggoled` without requiring root on linux you first need to copy [`11-steelseries-arctis-nova.rules`](https://github.com/JerwuQu/ggoled/blob/master/11-steelseries-arctis-nova.rules) into `/etc/udev/rules.d/` and run `udevadm control --reload` and `udevadm trigger` as root.

## CLI usage examples

See `ggoled --help` for all commands and flags.

- `ggoled brightness 1`: set the brightness to low.
- `ggoled text "Hello, World!"`: draw some text onto the display.
- `ggoled img cool_image.png`: draw an image onto the display.
- `ggoled anim -r 10 -l 20 frame1.png frame2.png frame3.png`: play an animation at 10 fps, looped 20 times.
- `ggoled anim animation.gif`: play a gif animation.

You also can play video animations by first extracting frames with `ffmpeg`:

```sh
ffmpeg -i YOURVIDEO.mp4 -r 20 -vf "scale=w=128:h=64:force_original_aspect_ratio=1" frames/%05d.png
ggoled anim -r 20 frames/*  # bash
ggoled anim -r 20 $(Get-ChildItem frames | % { $_.FullName })  # powershell
```

## Desktop application

The application puts itself as an icon in the system tray that you can right-click to configure.

For Windows, it gets media information from the Windows API which makes it work with almost all applications (with some limitations).

There are also features to avoid OLED burn-in that is otherwise unavoidable when using the official software, such as the screensaver function which will turn off the OLED display when away from the computer, or the OLED shifter which will infrequently move things around slightly.
To extend the lifespan of your display, both of these are strongly recommended to use, along with using a low screen brightness.

### systemd service

```sh
mkdir -p ~/.config/systemd/user/
cp ggoled_app.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now ggoled_app.service
``` 

### Custom font

It's recommended to use bitmap fonts to avoid weird artifacting, but any TTF or OTF font should work.

Modify the `%appdata%\ggoled_app.toml` file and add:

```toml
[font]
path = 'C:\Path\To\Font.ttf'
size = 16.0
```

Then restart the application.
