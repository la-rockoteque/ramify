"""Render bonsai.obj to shaded ASCII art and write src/bonsai_art.rs.

Usage: python3 assets/bonsai/ascii_bonsai.py [--yaw -25] [--preview 44x15]

Each cell gets a character from RAMP (dark to bright) and an ink code naming
what it shows: t = bark, d = deadwood, l = leaves, m = moss, g = ground,
s = sun. The Rust side colours each cell by its ink and brightness.
It writes one render per entry in SIZES. --preview prints one size with ANSI\ncolours instead of writing the Rust file.
"""
import argparse
import math
import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
RAMP = " .:-=+*#%@"
INK = {"bark": "t", "deadwood": "d", "leaves": "l", "moss": "m", "soil": "g"}
CELL_ASPECT = 2.0  # a terminal cell is about twice as tall as it is wide
SS = 3             # supersamples per cell axis


def load_obj(path):
    verts, tris = [], []
    material = None
    for line in open(path):
        p = line.split()
        if not p:
            continue
        if p[0] == "v":
            verts.append(tuple(float(x) for x in p[1:4]))
        elif p[0] == "usemtl":
            material = p[1]
        elif p[0] == "f":
            idx = [int(x.split("/")[0]) - 1 for x in p[1:]]
            for k in range(1, len(idx) - 1):
                tris.append((idx[0], idx[k], idx[k + 1], material))
    return verts, tris


def sub(a, b): return (a[0] - b[0], a[1] - b[1], a[2] - b[2])
def cross(a, b): return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])
def dot(a, b): return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
def norm(a):
    l = math.sqrt(dot(a, a)) or 1.0
    return (a[0] / l, a[1] / l, a[2] / l)


def vertex_normals(verts, tris):
    acc = [[0.0, 0.0, 0.0] for _ in verts]
    for a, b, c, _ in tris:
        n = cross(sub(verts[b], verts[a]), sub(verts[c], verts[a]))
        for i in (a, b, c):
            acc[i][0] += n[0]; acc[i][1] += n[1]; acc[i][2] += n[2]
    return [norm(tuple(n)) for n in acc]


def render(cols, rows, yaw_deg):
    verts, tris = load_obj(os.path.join(HERE, "bonsai.obj"))
    normals = vertex_normals(verts, tris)

    # Turn the model about Y; the camera looks down -Z, slightly from above.
    yaw, pitch = math.radians(yaw_deg), math.radians(-8)
    cy, sy, cp, sp = math.cos(yaw), math.sin(yaw), math.cos(pitch), math.sin(pitch)

    def view(p):
        x, y, z = p
        x, z = x * cy + z * sy, -x * sy + z * cy
        y, z = y * cp - z * sp, y * sp + z * cp
        return (x, y, z)

    vv = [view(p) for p in verts]
    vn = [view(n) for n in normals]

    # Fit the model into the grid, leaving room top-right for the sun.
    xs, ys = [p[0] for p in vv], [p[1] for p in vv]
    x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    W, H = cols * SS, rows * SS
    scale = min((W - 2) / (x1 - x0), (H - 2) * CELL_ASPECT / (y1 - y0))
    ox = (W - (x1 - x0) * scale) / 2 - x0 * scale
    oy = (H - 1) + y0 * scale / CELL_ASPECT

    def to_screen(p):
        return (p[0] * scale + ox, oy - p[1] * scale / CELL_ASPECT, p[2])

    sv = [to_screen(p) for p in vv]
    zbuf = [[-1e9] * W for _ in range(H)]
    shade = [[None] * W for _ in range(H)]
    light = norm((-0.5, 0.7, 0.6))

    for a, b, c, mat in tris:
        A, B, C = sv[a], sv[b], sv[c]
        area = (B[0] - A[0]) * (C[1] - A[1]) - (B[1] - A[1]) * (C[0] - A[0])
        if abs(area) < 1e-9:
            continue
        minx, maxx = max(int(min(A[0], B[0], C[0])), 0), min(int(max(A[0], B[0], C[0])) + 1, W - 1)
        miny, maxy = max(int(min(A[1], B[1], C[1])), 0), min(int(max(A[1], B[1], C[1])) + 1, H - 1)
        for py in range(miny, maxy + 1):
            for px in range(minx, maxx + 1):
                x, y = px + 0.5, py + 0.5
                w0 = ((B[0] - x) * (C[1] - y) - (B[1] - y) * (C[0] - x)) / area
                w1 = ((C[0] - x) * (A[1] - y) - (C[1] - y) * (A[0] - x)) / area
                w2 = 1 - w0 - w1
                if w0 < 0 or w1 < 0 or w2 < 0:
                    continue
                z = w0 * A[2] + w1 * B[2] + w2 * C[2]
                if z <= zbuf[py][px]:
                    continue
                zbuf[py][px] = z
                n = norm(tuple(w0 * vn[a][k] + w1 * vn[b][k] + w2 * vn[c][k] for k in range(3)))
                lum = 0.18 + 0.82 * max(0.0, dot(n, light))
                shade[py][px] = (lum, INK.get(mat, "t"))

    # Sun: an inked disc behind the tree, upper right, rough at the rim.
    rnd = random.Random(3)
    sun_cx, sun_cy, sun_r = W * 0.70, H * 0.28, H * 0.27
    art = []
    for r in range(rows):
        chars, inks = [], []
        for c in range(cols):
            hits = [shade[r * SS + j][c * SS + i] for j in range(SS) for i in range(SS)]
            hits = [h for h in hits if h]
            if len(hits) >= SS * SS / 3:
                lum = sum(h[0] for h in hits) / len(hits)
                lum *= 0.6 + 0.4 * len(hits) / (SS * SS)  # thin edges read lighter
                ink = max(set(h[1] for h in hits), key=lambda k: sum(1 for h in hits if h[1] == k))
                chars.append(RAMP[max(1, min(len(RAMP) - 1, round(lum * (len(RAMP) - 1))))])
                inks.append(ink)
                continue
            dx = (c + 0.5) * SS - sun_cx
            dy = ((r + 0.5) * SS - sun_cy) * CELL_ASPECT
            d = math.hypot(dx, dy) / (sun_r * CELL_ASPECT)
            if d < 1.0 and rnd.random() > (d ** 6) * 0.8:
                level = 3 + int((1 - d) * 3 + rnd.random() * 1.5)
                chars.append(RAMP[min(level, 6)])
                inks.append("s")
            else:
                chars.append(" ")
                inks.append(" ")
        art.append(("".join(chars).rstrip(), "".join(inks)[: len("".join(chars).rstrip())]))
    while art and not art[0][0]:
        art.pop(0)
    return art


ANSI = {"t": (150, 128, 108), "d": (214, 204, 186), "l": (70, 140, 100), "m": (110, 150, 70),
        "g": (120, 96, 72), "s": (200, 60, 50)}


def preview(art):
    for chars, inks in art:
        out = []
        for ch, ink in zip(chars, inks):
            if ink == " ":
                out.append(ch)
                continue
            f = 0.45 + 0.55 * RAMP.index(ch) / (len(RAMP) - 1)
            r, g, b = (int(v * f) for v in ANSI[ink])
            out.append(f"\x1b[38;2;{r};{g};{b}m{ch}")
        print("".join(out) + "\x1b[0m")


SIZES = ((64, 26), (44, 15))  # (cols, rows), largest first


def write_rust(yaw):
    path = os.path.join(REPO, "src", "bonsai_art.rs")
    with open(path, "w") as f:
        f.write(f"// @generated by assets/bonsai/ascii_bonsai.py --yaw {yaw}. Do not edit by hand.\n")
        f.write("//! Splash bonsai: shaded ASCII renders of assets/bonsai/bonsai.obj.\n\n")
        f.write("/// Characters from dark to bright.\n")
        f.write(f'pub const RAMP: &str = "{RAMP}";\n\n')
        f.write("/// One render: rows of (characters, inks).\n")
        f.write("/// Inks: t bark, d deadwood, l leaves, m moss, g ground, s sun.\n")
        f.write("pub type Art = &'static [(&'static str, &'static str)];\n\n")
        f.write("/// Renders from largest to smallest.\n")
        f.write("#[rustfmt::skip]\npub const ARTS: &[Art] = &[\n")
        for cols, rows in SIZES:
            art = render(cols, rows, yaw)
            f.write("    &[\n")
            for chars, inks in art:
                esc = chars.replace("\\", "\\\\").replace('"', '\\"')
                f.write(f'        ("{esc}", "{inks}"),\n')
            f.write("    ],\n")
            print(f"{len(art)} rows x {max(len(c) for c, _ in art)} cols")
        f.write("];\n")
    print(f"wrote {os.path.relpath(path, REPO)}")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--yaw", type=float, default=-25)
    ap.add_argument("--preview", metavar="COLSxROWS", help="print one size with ANSI colours, e.g. 44x15")
    args = ap.parse_args()
    if args.preview:
        cols, rows = (int(v) for v in args.preview.split("x"))
        preview(render(cols, rows, args.yaw))
    else:
        write_rust(args.yaw)
