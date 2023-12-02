#!/bin/sh
# scripts/clock.sh
while true; do
	"$(dirname "$0")/textpng.sh" "$(date +"%Y-%m-%d %H:%M:%S")"
	sleep 1
done
