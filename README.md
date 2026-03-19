# ggoled

Put custom graphics on your SteelSeries Arctis Nova Pro Base Station 128x64 OLED screen.

This utility implements the USB protocol, so you don't need SteelSeries GG/Engine Apps/GameSense, and it works on linux.

There is also a [desktop application](#desktop-application) available for Windows, Linux, and MacOS that shows the current time and currently playing media, along with some other features.

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

Pre-built binaries are available from [GitHub Actions](https://github.com/JerwuQu/ggoled/actions?query=branch%3Amaster) or via nightly.link.

### Windows

1. Download: [x86_64-pc-windows-gnu.zip (via nightly.link)](https://nightly.link/JerwuQu/ggoled/workflows/build/master/x86_64-pc-windows-gnu.zip)
2. Extract and run `ggoled_app.exe`.

### Linux (Flatpak)

1. Download: [ggoled-x86_64.flatpak.zip (via nightly.link)](https://nightly.link/JerwuQu/ggoled/workflows/build/master/ggoled-x86_64.flatpak.zip)
2. Extract the zip and install: `flatpak install ggoled-x86_64.flatpak`
3. Install the udev rules so the device is accessible without root:
   1. Copy [`11-steelseries-arctis-nova.rules`](https://github.com/JerwuQu/ggoled/blob/master/11-steelseries-arctis-nova.rules) into `/etc/udev/rules.d/`
   2. Run `sudo udevadm control --reload && sudo udevadm trigger`.
4. Launch via your application menu or via `flatpak run se.ramse.ggoled`.

The CLI is also available: `flatpak run --command=ggoled se.ramse.ggoled text Hello!`.

### From source

1. Install the Rust toolchain.
2. Build directly from git, either:
   - **A:** Using installed SDL3: `cargo install --locked --git https://github.com/JerwuQu/ggoled.git ggoled ggoled_app`
   - **B:** Building SDL3 from source: `cargo install --locked --git https://github.com/JerwuQu/ggoled.git --features sdl3-static ggoled ggoled_app`
   - **C:** CLI only: `cargo install --locked --git https://github.com/JerwuQu/ggoled.git ggoled`
3. (_Linux only_) Install the udev rules as described in the [flatpak section](#linux-flatpak) above to run without root.
4. (_Linux only, optional_) Install the systemd service: see below.

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

Modify the config file (`%appdata%\ggoled_app.toml` on Windows, `~/.config/ggoled_app.toml` on Linux) and add:

```toml
[font]
path = 'C:\Path\To\Font.ttf'
size = 16.0
```

Then restart the application.
