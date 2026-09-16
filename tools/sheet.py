# python tools/sheet.py <prefix> <out.png> [ref cells...]: tiles poseshot output with labels
import sys, json
from PIL import Image, ImageDraw
prefix, out = sys.argv[1], sys.argv[2]
refs = sys.argv[3:]
labels = json.load(open(prefix + ".json", encoding="utf8"))
tiles = []
for i, l in enumerate(labels):
    im = Image.open(f"{prefix}.{i}.png").convert("RGBA")
    bg = Image.new("RGBA", im.size, "white"); bg.alpha_composite(im); im = bg.convert("RGB").resize((260, 260))
    if i < len(refs):
        r = Image.open(refs[i]).convert("RGB"); r = r.resize((int(r.width * 260 / r.height), 260))
        t = Image.new("RGB", (260 + r.width, 280), "white"); t.paste(r, (0, 0)); t.paste(im, (r.width, 0)); im = t
    else:
        t = Image.new("RGB", (260, 280), "white"); t.paste(im, (0, 0)); im = t
    ImageDraw.Draw(im).text((6, 264), l, fill="black")
    tiles.append(im)
cols = 4 if refs else 6
w = max(t.width for t in tiles); rows = (len(tiles) + cols - 1) // cols
sheet = Image.new("RGB", (w * cols, 280 * rows), "#ddd")
for i, t in enumerate(tiles):
    sheet.paste(t, ((i % cols) * w, (i // cols) * 280))
sheet.save(out)
