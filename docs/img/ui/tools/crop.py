#!/usr/bin/env python3
"""Crop (and optionally zoom) a PNG for close inspection.

    python3 tools/crop.py in.png out.png x y w h [zoom]
"""
import sys

import cairo

src, dst = sys.argv[1], sys.argv[2]
x, y, w, h = (int(v) for v in sys.argv[3:7])
zoom = float(sys.argv[7]) if len(sys.argv) > 7 else 1.0
image = cairo.ImageSurface.create_from_png(src)
out = cairo.ImageSurface(cairo.FORMAT_RGB24, int(w * zoom), int(h * zoom))
ctx = cairo.Context(out)
ctx.scale(zoom, zoom)
ctx.set_source_surface(image, -x, -y)
ctx.get_source().set_filter(cairo.FILTER_BEST)
ctx.paint()
out.write_to_png(dst)
