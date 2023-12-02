#!/bin/sh
# scripts/textpng.sh "Hello, World!"
magick convert -size 128x64 canvas:black -pointsize 16 -fill white -font "$(dirname "$0")/PixelOperator.ttf" -draw "text 0,12 '$1'" png:- | sanpwo img -

