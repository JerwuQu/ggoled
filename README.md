# ggoled

Put custom graphics on your SteelSeries Arctis Nova Pro Base Station 128x64 OLED screen.

This utility implements the USB protocol, so you don't need SteelSeries GG/Engine Apps/GameSense, and it works on linux.

It is currently focused on the SteelSeries Arctis Nova product(s) since that's what I own myself, but PRs and issues for other models (or brands!) are welcome!

## Showcase

Bad Apple at 60 fps.
This also showcases the burn-in you will get if not careful with OLEDs. The flickering is due to bad camera settings and not actually shown on the display.

[![Bad Apple on the Base Station](http://img.youtube.com/vi/k51zNrMLti4/0.jpg)](http://www.youtube.com/watch?v=k51zNrMLti4 "Bad Apple on a SteelSeries Arctis Nova Pro Wireless Base Station")

## Usage

- `ggoled text "Hello, World!"`: draw some text onto your display.
- `ggoled img cool_image.png`: draw an image onto your display.
- `ggoled anim -r 10 -l 20 frame1.png frame2.png frame3.png`: play an animation at 10 fps, looped 20 times.
- See `ggoled --help` for more commands and flags.

## Install

`cargo install --git https://github.com/JerwuQu/ggoled.git`

To run without root on linux you need to copy [`11-steelseries-arctis-nova.rules`](https://github.com/JerwuQu/ggoled/blob/master/11-steelseries-arctis-nova.rules) into `/etc/udev/rules.d/` and run `udevadm control --reload` and `udevadm trigger` as root.
