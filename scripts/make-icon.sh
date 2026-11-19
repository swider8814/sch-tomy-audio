#!/usr/bin/env bash
# Generates the app icon source (1024x1024 PNG) with ffmpeg, then all
# platform icons with the Tauri CLI. Run from the repository root.
set -euo pipefail
out=src-tauri/icons/source.png
bars=""
heights=(180 320 520 700 460 820 600 380 640 300 200)
x=170
for h in "${heights[@]}"; do
  y=$(( (1024 - h) / 2 ))
  bars+="drawbox=x=${x}:y=${y}:w=48:h=${h}:color=white@1:t=fill,"
  x=$(( x + 64 ))
done
ffmpeg -hide_banner -loglevel error -y -f lavfi -i "color=c=0x1f4e79:s=1024x1024,format=rgba" \
  -vf "${bars}geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='if(lte(hypot(max(abs(X-511.5)-312,0),max(abs(Y-511.5)-312,0)),200),255,0)'" \
  -frames:v 1 "$out"
npx tauri icon "$out" -o src-tauri/icons
