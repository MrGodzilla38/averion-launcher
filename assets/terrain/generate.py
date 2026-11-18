"""Rebuild Averion's original terrain plates with Python and ImageMagick 7.

Authoring only. No third-party imagery, textures, models, fonts or game data.
"""
import math
import random
import subprocess
from pathlib import Path

ROOT = Path(__file__).parent
WIDTH, HEIGHT = 512, 256
SCENES = {
    "tundra": ("#537ea1", "#ffd3a7", "#6c96a7", "#b1c8cd", "#769181"),
    "grove": ("#64639b", "#ffcbae", "#7d91a1", "#cd91a3", "#779d88"),
    "delta": ("#477590", "#ffcd91", "#5b9e9d", "#bf9c78", "#62a08b"),
    "fallback": ("#495b77", "#b3cad2", "#708da2", "#a0b8bf", "#708d91"),
}


def tint(color, factor):
    values = bytes.fromhex(color[1:])
    return "#" + "".join(f"{min(255, int(c * factor)):02x}" for c in values)


def render(name, seed, colors):
    sky, sun, water, stone, grass = colors
    rng = random.Random(seed)
    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}">',
             f'<defs><linearGradient id="sky" x2="0" y2="1"><stop stop-color="{sky}"/><stop offset="1" stop-color="{sun}"/></linearGradient></defs>',
             '<rect width="512" height="256" fill="url(#sky)"/>',
             f'<rect x="370" y="35" width="23" height="23" fill="{sun}"/>']

    def polygon(points, color):
        points = " ".join(f"{x:.1f},{y:.1f}" for x, y in points)
        parts.append(f'<polygon points="{points}" fill="{color}"/>')

    def block(x, y, width, depth, height, color):
        dx, dy = width, depth
        polygon([(x-dx,y),(x,y+dy),(x,y+dy+height),(x-dx,y+height)], tint(color,.73))
        polygon([(x+dx,y),(x,y+dy),(x,y+dy+height),(x+dx,y+height)], tint(color,.52))
        polygon([(x,y-dy),(x+dx,y),(x,y+dy),(x-dx,y)], color)

    for layer in range(3):
        points = [(-8,HEIGHT)]
        for x in range(-8,WIDTH+9,8):
            y = 84 + 14*layer + 16*math.sin(x/59+seed+layer) + 9*math.sin(x/22+layer)
            level = int(y/4)*4
            points.extend([(x,level),(x+8,level)])
        polygon(points+[(WIDTH+8,HEIGHT)], tint(water,1.28-.16*layer))

    for row in range(42):
        for col in range(-1,34):
            x = col*18 + (row%2)*9
            y = 124 + row*4.3
            river = 265+70*math.sin(row/11+seed)
            bank = abs(x-river) - (22+row*.9)
            relief = (bank/3.7 + 10*math.sin(x/37+row*.13) +
                      7*math.sin(col*.6+row*.38))
            h = 0 if bank<0 else max(0, int(relief/5)*5)
            top = grass if h<40 else stone
            if name=="tundra" and h>42:
                top="#d7e4dc"
            if h==0:
                block(x,y,9.2,4.4,1,tint(water,rng.uniform(.92,1.08)))
                continue
            block(x,y-h,9.2,4.4,h+2,tint(top,rng.uniform(.92,1.05)))
            if rng.random()<.07 and h<48:
                block(x,y-h-4,1.8,1,4,"#7c6558")
                leaves = "#d7a6b4" if name=="grove" else "#3d7067"
                block(x,y-h-9,5.5,2.6,5,leaves)
                block(x,y-h-13,3.6,1.8,4,tint(leaves,1.1))
    svg = "".join(parts) + "</svg>"
    for suffix, args in [("card", []), ("sky", ["-resize","256x128!", "-blur","0x2", "-fill","#101b32","-colorize","52%"])]:
        subprocess.run(["magick","-background","none","svg:-",*args,"-depth","8",
                        "rgba:"+str(ROOT/f"{name}-{suffix}.rgba")],input=svg.encode(),check=True)


if __name__ == "__main__":
    for index,(name,palette) in enumerate(SCENES.items()):
        render(name, index+3, palette)
