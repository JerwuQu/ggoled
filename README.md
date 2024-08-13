# arctis-nova-oled

Put graphics on your SteelSeries Arctis Nova Pro Base Station 128x64 OLED screen.

This utility implements the USB protocol, so you don't need SteelSeries GG/Engine Apps/GameSense, and it works on linux.

## Showcase

Bad Apple at 60 fps.
This also showcases the burn-in you will get if not careful with OLEDs. The flickering is due to bad camera settings and not actually shown on the display.

[![Bad Apple on the Base Station](http://img.youtube.com/vi/k51zNrMLti4/0.jpg)](http://www.youtube.com/watch?v=k51zNrMLti4 "Bad Apple on a SteelSeries Arctis Nova Pro Wireless Base Station")

## Usage

- `sanpwo text "Hello, World!"`: draw some text onto your display.
- `sanpwo img cool_image.png`: draw an image onto your display.
- `sanpwo anim -r 10 -l 20 frame1.png frame2.png frame3.png`: play an animation at 10 fps, looped 20 times.
- See `sanpwo --help` for more help, and [the `scripts`](https://github.com/JerwuQu/arctis-nova-oled/tree/master/scripts) for more examples.

## Install

`cargo install --git https://github.com/JerwuQu/arctis-nova-oled.git`

To run without root on linux you need to copy [`11-steelseries-arctis-nova.rules`](https://github.com/JerwuQu/arctis-nova-oled/blob/master/11-steelseries-arctis-nova.rules) into `/etc/udev/rules.d/` and run `udevadm control --reload` and `udevadm trigger` as root.
