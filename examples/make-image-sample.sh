#!/usr/bin/env bash
# 產生 image-text-sample.zip：6 張用 ImageMagick 畫的示意圖（自己畫的，CC0）和一份 items.csv。
# 需要 ImageMagick 7（magick）和 python3。在 examples/ 資料夾裡執行：bash make-image-sample.sh
set -euo pipefail
here=$(pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/images"
cd "$work/images"

magick -size 480x320 gradient:'#7dd3fc-#e0f2fe' \( -size 480x120 xc:'#0284c7' \) -gravity south -composite \
  -fill '#facc15' -draw 'circle 360,90 360,135' -fill '#fde68a' -draw 'rectangle 0,300 480,320' beach.png
magick -size 480x320 xc:'#fff7ed' -fill '#f59e0b' -stroke '#b45309' -strokewidth 4 \
  -draw 'path "M 120,170 Q 125,280 240,280 Q 355,280 360,170 Z"' \
  -fill none -stroke '#fbbf24' -strokewidth 6 -draw 'path "M 150,175 Q 180,150 210,175 T 270,175 T 330,175"' \
  -stroke '#cbd5e1' -strokewidth 5 -draw 'path "M 200,140 Q 185,110 200,80"' \
  -draw 'path "M 240,140 Q 225,110 240,80"' -draw 'path "M 280,140 Q 265,110 280,80"' noodles.png
magick -size 480x320 xc:'#f1f5f9' -fill '#94a3b8' -draw 'polygon 170,110 195,40 235,100' \
  -draw 'polygon 245,100 285,40 310,110' -draw 'circle 240,170 240,80' -fill '#0f172a' \
  -draw 'circle 205,160 205,172' -draw 'circle 275,160 275,172' -fill '#f472b6' -draw 'polygon 232,190 248,190 240,200' \
  -stroke '#0f172a' -strokewidth 2 -draw 'line 150,195 210,200' -draw 'line 270,200 330,195' cat.png
magick -size 480x320 xc:'#ecfeff' -fill '#334155' -draw 'roundrectangle 130,70 350,220 10,10' \
  -fill '#22d3ee' -draw 'rectangle 145,85 335,205' -fill '#475569' -draw 'polygon 100,230 380,230 400,255 80,255' laptop.png
magick -size 480x320 gradient:'#bae6fd-#f0f9ff' -fill '#64748b' -draw 'polygon 0,300 140,110 280,300' \
  -draw 'polygon 160,300 320,80 480,300' -fill '#ffffff' -draw 'polygon 115,145 140,110 165,145' \
  -draw 'polygon 285,128 320,80 355,128' -fill '#16a34a' -draw 'rectangle 0,290 480,320' mountain.png
magick -size 480x320 xc:'#cbd5e1' -fill '#64748b' -draw 'circle 200,110 200,60' -draw 'circle 280,100 280,40' \
  -draw 'circle 330,125 330,85' -draw 'roundrectangle 150,100 370,160 30,30' -stroke '#0ea5e9' -strokewidth 4 \
  -draw 'line 180,180 165,220' -draw 'line 230,180 215,220' -draw 'line 280,180 265,220' -draw 'line 330,180 315,220' \
  -draw 'line 205,240 190,280' -draw 'line 255,240 240,280' -draw 'line 305,240 290,280' rain.png
for f in *.png; do magick "$f" -colors 32 +dither -strip "PNG8:$f"; done

cd "$work"
cat > items.csv <<'CSV'
text,image
週末去海邊，太陽好大,images/beach.png
巷口那家拉麵終於排到了,images/noodles.png
我家的貓今天又打翻水杯,images/cat.png
新買的筆電開箱，螢幕好亮,images/laptop.png
今天的晚餐是牛排,images/mountain.png
出門忘了帶傘，只好在便利商店躲雨,images/rain.png
CSV
out="$here/image-text-sample.zip"
rm -f "$out"
python3 -m zipfile -c "$out" items.csv images
echo "wrote $out"
