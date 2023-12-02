#!/bin/bash
# scripts/scroll.sh <img>
HEIGHT=$(magick identify -ping -format '%h' "$1")
for ((y = 0; y <= HEIGHT - 64; y++)); do
	magick convert "$1" -crop 128x64+0+$y "/tmp/sanpwo-scroll-$y.png"
done
ANIM=""
for ((y = 0; y < HEIGHT - 64; y++)); do
	ANIM="$ANIM /tmp/sanpwo-scroll-$y.png"
done
for ((y = HEIGHT - 64; y > 0; y--)); do
	ANIM="$ANIM /tmp/sanpwo-scroll-$y.png"
done
sanpwo anim --clear --loops 0 --framerate 20 $ANIM
