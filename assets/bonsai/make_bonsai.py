"""Generate a stylised bonsai as bonsai.obj + bonsai.mtl (pure Python, no deps).

Units: 1 unit = 1 cm. The model stands on y = 0, Y is up.
Run from any folder; files are written next to this script.
"""
import math
import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))

random.seed(7)

# ---------------------------------------------------------------- vec helpers
def add(a, b): return (a[0] + b[0], a[1] + b[1], a[2] + b[2])
def sub(a, b): return (a[0] - b[0], a[1] - b[1], a[2] - b[2])
def mul(a, s): return (a[0] * s, a[1] * s, a[2] * s)
def dot(a, b): return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
def cross(a, b): return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])
def norm(a):
    l = math.sqrt(dot(a, a)) or 1.0
    return (a[0] / l, a[1] / l, a[2] / l)


# ---------------------------------------------------------------- mesh store
class Obj:
    def __init__(self, name, material):
        self.name, self.material = name, material
        self.v, self.f = [], []

    def vert(self, p):
        self.v.append(p)
        return len(self.v) - 1

    def face(self, idx, inside):
        """Add a polygon; flip it so its normal points away from `inside`."""
        a, b, c = (self.v[i] for i in idx[:3])
        n = cross(sub(b, a), sub(c, a))
        centre = mul(tuple(map(sum, zip(*(self.v[i] for i in idx)))), 1 / len(idx))
        if dot(n, sub(centre, inside)) < 0:
            idx = list(reversed(idx))
        self.f.append(list(idx))


objects = []


def new(name, material):
    o = Obj(name, material)
    objects.append(o)
    return o


# ---------------------------------------------------------------- primitives
def box(o, lo, hi, top_shrink=(0.0, 0.0)):
    """Axis-aligned box; top face can be shrunk (sx, sz) for a taper."""
    (x0, y0, z0), (x1, y1, z1) = lo, hi
    sx, sz = top_shrink
    pts = [(x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1),
           (x0 + sx, y1, z0 + sz), (x1 - sx, y1, z0 + sz), (x1 - sx, y1, z1 - sz), (x0 + sx, y1, z1 - sz)]
    i = [o.vert(p) for p in pts]
    c = ((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2)
    for q in ([0, 1, 2, 3], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]):
        o.face([i[k] for k in q], c)


def blob(o, centre, radii, seg=14, ring=8, jitter=0.12):
    """Lumpy ellipsoid used for foliage clumps."""
    rx, ry, rz = radii
    top = o.vert(add(centre, (0, ry, 0)))
    bot = o.vert(add(centre, (0, -ry * 0.7, 0)))
    rows = []
    for r in range(1, ring):
        phi = math.pi * r / ring
        row = []
        for s in range(seg):
            th = 2 * math.pi * s / seg
            k = 1 + random.uniform(-jitter, jitter)
            y = math.cos(phi) * ry * (1 if phi < math.pi / 2 else 0.7)
            row.append(o.vert(add(centre, (math.sin(phi) * math.cos(th) * rx * k, y * k,
                                           math.sin(phi) * math.sin(th) * rz * k))))
        rows.append(row)
    for s in range(seg):
        t = (s + 1) % seg
        o.face([top, rows[0][s], rows[0][t]], centre)
        o.face([bot, rows[-1][t], rows[-1][s]], centre)
        for r in range(len(rows) - 1):
            o.face([rows[r][s], rows[r + 1][s], rows[r + 1][t], rows[r][t]], centre)


def catmull(pts, n):
    """Sample a Catmull-Rom spline through pts with n points per segment."""
    p = [pts[0]] + pts + [pts[-1]]
    out = []
    for i in range(1, len(p) - 2):
        for k in range(n):
            t = k / n
            t2, t3 = t * t, t * t * t
            out.append(tuple(0.5 * (2 * p[i][j] + (-p[i - 1][j] + p[i + 1][j]) * t
                                    + (2 * p[i - 1][j] - 5 * p[i][j] + 4 * p[i + 1][j] - p[i + 2][j]) * t2
                                    + (-p[i - 1][j] + 3 * p[i][j] - 3 * p[i + 1][j] + p[i + 2][j]) * t3)
                         for j in range(3)))
    out.append(pts[-1])
    return out


def tube(o, path, r0, r1, sides=12, cap=True, flare=0.0):
    """Tapered tube along path, radius r0 -> r1; optional root flare at start."""
    rings, prev_n = [], None
    for i, p in enumerate(path):
        t = i / (len(path) - 1)
        d = norm(sub(path[min(i + 1, len(path) - 1)], path[max(i - 1, 0)]))
        if prev_n is None:
            up = (0, 0, 1) if abs(d[2]) < 0.9 else (1, 0, 0)
            prev_n = norm(cross(d, up))
        n = norm(sub(prev_n, mul(d, dot(prev_n, d))))  # parallel transport
        prev_n = n
        b = cross(d, n)
        r = r0 + (r1 - r0) * t + flare * max(0.0, 1 - t * 6) ** 2
        ring = []
        for s in range(sides):
            a = 2 * math.pi * s / sides
            k = r * (1 + 0.06 * math.sin(3 * a + i * 0.7))  # bark wobble
            ring.append(o.vert(add(p, add(mul(n, math.cos(a) * k), mul(b, math.sin(a) * k)))))
        rings.append((ring, p))
    for i in range(len(rings) - 1):
        (ra, pa), (rb, pb) = rings[i], rings[i + 1]
        mid = mul(add(pa, pb), 0.5)
        for s in range(sides):
            t = (s + 1) % sides
            o.face([ra[s], rb[s], rb[t], ra[t]], mid)
    if cap:
        ring, p = rings[-1]
        tip = o.vert(add(p, mul(norm(sub(path[-1], path[-2])), r1 * 0.6)))
        for s in range(sides):
            o.face([ring[s], ring[(s + 1) % sides], tip], path[-2])


def on_trunk(path, t):
    return path[int(t * (len(path) - 1))]


# ---------------------------------------------------------------- the model
# Shaped after assets/ramify-512.png: a windswept tree on a mossy mound.
# The base sits right of centre, the trunk twists left then up-right, bare
# deadwood (jin) reaches right, and the foliage pads sweep out to the left.
GROUND_Y = 0.0
ground = new("ground", "soil")
blob(ground, (6.0, GROUND_Y - 1.0, 0.0), (11.0, 2.4, 7.0), seg=18, ring=8, jitter=0.08)

moss = new("moss", "moss")
for _ in range(10):
    x, z = random.uniform(-2, 14), random.uniform(-5, 5)
    blob(moss, (x, GROUND_Y + 0.6, z), (random.uniform(1.0, 2.2), 0.7, random.uniform(1.0, 1.8)), seg=10, ring=5)

trunk = new("trunk", "bark")
trunk_pts = [(8.0, GROUND_Y, 0.0), (6.5, 5.0, 0.5), (2.5, 9.0, -0.5), (0.0, 13.0, 0.5),
             (1.5, 17.0, 0.0), (4.0, 21.0, -0.5), (5.5, 25.0, 0.0), (4.5, 28.5, 0.5)]
trunk_path = catmull(trunk_pts, 8)
tube(trunk, trunk_path, 3.2, 0.7, sides=18, flare=3.5)

# Surface roots (nebari), heavier on the right like the logo.
for ang, length in ((-10, 8), (30, 6), (90, 5), (160, 4), (210, 5), (300, 6)):
    a = math.radians(ang)
    dx, dz = math.cos(a), math.sin(a)
    root_path = catmull([(8.0, GROUND_Y + 1.5, 0), (8.0 + dx * length * 0.5, GROUND_Y + 0.3, dz * length * 0.5),
                         (8.0 + dx * length, GROUND_Y - 0.4, dz * length)], 5)
    tube(trunk, root_path, 1.4, 0.3, sides=8)

# Deadwood: pale, bare branches reaching right and up.
dead = new("deadwood", "deadwood")
for t, end, r in ((0.80, (13.0, 27.0, 0.0), 0.55), (0.92, (10.0, 31.5, -1.0), 0.40),
                  (0.70, (11.5, 22.0, 1.5), 0.45), (0.98, (7.0, 33.0, 0.5), 0.30)):
    start = on_trunk(trunk_path, t)
    mid = add(mul(add(start, end), 0.5), (0.0, 1.2, 0.0))
    tube(dead, catmull([start, mid, end], 5), r, 0.08, sides=6)

# Branches and foliage pads: (trunk t, branch end, pad radii).
foliage = new("foliage", "leaves")
branches = [
    (0.30, (-15.0, 9.0, 1.0), (7.5, 3.8, 5.5)),    # low pad, far left
    (0.48, (-10.0, 15.5, -1.5), (8.0, 4.2, 6.0)),  # middle left pad
    (0.66, (-4.0, 21.5, 1.0), (7.5, 4.0, 5.5)),    # upper left pad
    (0.88, (1.0, 28.0, -0.5), (9.0, 4.6, 6.5)),    # crown
    (0.56, (-1.0, 15.0, 3.5), (4.5, 3.0, 4.0)),    # small inner pad
]
pads = []
for t, end, radii in branches:
    start = on_trunk(trunk_path, t)
    mid = add(mul(add(start, end), 0.5), (0.0, 1.5, 0.0))
    tube(trunk, catmull([start, mid, end], 6), 1.0 * (1.2 - t), 0.25, sides=10)
    d = norm(sub(end, start))
    for k in (-1, 1):  # sub-branches spread into the pad
        side = norm((-d[2] * k + 0.01, 0, d[0] * k))
        twig_end = add(end, add(mul(side, radii[2] * 0.5), (0, 0.5, 0)))
        tube(trunk, catmull([mid, add(mid, (0, 1.0, 0)), twig_end], 4), 0.35, 0.15, sides=6)
    pads.append((add(end, (0, 1.0, 0)), radii))

for centre, (rx, ry, rz) in pads:
    blob(foliage, centre, (rx * 0.8, ry * 0.8, rz * 0.8))
    for _ in range(10):
        a = random.uniform(0, 2 * math.pi)
        rr = random.uniform(0.35, 0.9)
        c = add(centre, (math.cos(a) * rx * rr, random.uniform(-0.2, 0.6) * ry, math.sin(a) * rz * rr))
        s = random.uniform(0.35, 0.55)
        blob(foliage, c, (rx * s, ry * s * 1.1, rz * s), seg=10, ring=6, jitter=0.18)


# ---------------------------------------------------------------- write files
MATERIALS = {
    "soil": (0.23, 0.17, 0.12),
    "moss": (0.35, 0.50, 0.20),
    "bark": (0.42, 0.36, 0.30),
    "deadwood": (0.78, 0.74, 0.66),
    "leaves": (0.17, 0.33, 0.24),
}

with open(os.path.join(HERE, "bonsai.mtl"), "w") as m:
    for name, (r, g, b) in MATERIALS.items():
        m.write(f"newmtl {name}\nKa 0 0 0\nKd {r:.3f} {g:.3f} {b:.3f}\nKs 0.05 0.05 0.05\nNs 10\nd 1\nillum 2\n\n")

base, nv, nf = 1, 0, 0
with open(os.path.join(HERE, "bonsai.obj"), "w") as f:
    f.write("# Stylised bonsai, units = cm, Y up\nmtllib bonsai.mtl\n")
    for o in objects:
        f.write(f"\no {o.name}\nusemtl {o.material}\n")
        for x, y, z in o.v:
            f.write(f"v {x:.4f} {y:.4f} {z:.4f}\n")
        for face in o.f:
            f.write("f " + " ".join(str(i + base) for i in face) + "\n")
        base += len(o.v)
        nv += len(o.v)
        nf += len(o.f)

print(f"bonsai.obj: {len(objects)} objects, {nv} vertices, {nf} faces")
