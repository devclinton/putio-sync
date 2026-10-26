#!/usr/bin/env python3
"""Generate the accent themes for app.css (feature 012) and check their contrast.

Each accent tints the neutrals (background, surfaces, text, muted text, borders) by mixing a
little of the accent into a neutral base in OKLab. Prints CSS rules to paste into app.css;
exits non-zero if any text pair falls under WCAG AA.

    python3 specs/012-ui-themes/themes.py
"""
import sys

# name: (light accent, dark accent, tint strength); light accents carry white text,
# dark accents carry dark text
ACCENTS = {
    "blue": ("#0a64d2", "#5aa2ff", 1.0),
    "indigo": ("#5046d8", "#a39dff", 1.0),
    "teal": ("#09756f", "#3cc8be", 1.0),
    "green": ("#17723a", "#5ccf86", 0.9),
    "orange": ("#a54a08", "#ff9e4a", 0.8),
    "pink": ("#be2468", "#ff80b8", 0.9),
    "graphite": ("#4a4f5c", "#c4c8d2", 0.4),
}

# neutral base and tint percentage per token, (light, dark)
NEUTRALS = {
    "bg": (("#f5f5f7", 5), ("#121214", 6)),
    "surface": (("#ffffff", 1.2), ("#1c1c20", 6)),
    "surface-2": (("#ececf0", 7), ("#28282e", 9)),
    "line": (("#dcdce2", 10), ("#34343b", 10)),
    "text": (("#17171a", 12), ("#f2f2f5", 3)),
    "muted": (("#5c5c66", 14), ("#a4a4ae", 12)),
}
STATUS = {  # (light, dark)
    "ok": ("#17783f", "#55d08f"),
    "warn": ("#925a00", "#f0b544"),
    "bad": ("#bb2b27", "#ff8178"),
}
LIGHT_ON = "#ffffff"


def hex2rgb(h):
    h = h.lstrip("#")
    return [int(h[i : i + 2], 16) / 255 for i in (0, 2, 4)]


def rgb2hex(c):
    return "#" + "".join(f"{round(max(0, min(1, v)) * 255):02x}" for v in c)


def lin(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def delin(v):
    return 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055


def to_oklab(c):
    r, g, b = (lin(v) for v in c)
    l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b) ** (1 / 3)
    m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b) ** (1 / 3)
    s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b) ** (1 / 3)
    return [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]


def from_oklab(c):
    L, a, b = c
    l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s = (L - 0.0894841775 * a - 1.2914855480 * b) ** 3
    return [
        delin(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        delin(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        delin(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
    ]


def tint(base, accent, pct):
    """Keep the base's lightness, take `pct`% of the accent's hue and chroma"""
    b, a = to_oklab(hex2rgb(base)), to_oklab(hex2rgb(accent))
    t = pct / 100
    return rgb2hex(from_oklab([b[0], b[1] + (a[1] - b[1]) * t, b[2] + (a[2] - b[2]) * t]))


def mix(fg, bg, t):
    """sRGB mix, as color-mix(in srgb, fg t, bg)"""
    f, g = hex2rgb(fg), hex2rgb(bg)
    return rgb2hex([f[i] * t + g[i] * (1 - t) for i in range(3)])


def lum(h):
    r, g, b = (lin(v) for v in hex2rgb(h))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    la, lb = sorted((lum(a), lum(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def theme(name, mode):
    i = 0 if mode == "light" else 1
    accent = ACCENTS[name][i]
    strength = ACCENTS[name][2]
    t = {k: tint(v[i][0], accent, v[i][1] * strength) for k, v in NEUTRALS.items()}
    t["accent"] = accent
    t["on-accent"] = LIGHT_ON if mode == "light" else tint("#101014", accent, 20)
    for k, v in STATUS.items():
        t[k] = v[i]
    return t


def check(name, mode, t):
    failures = []
    surfaces = ["bg", "surface", "surface-2"]
    for fg in ["text", "muted", "accent", "ok", "warn", "bad"]:
        for bg in surfaces:
            c = contrast(t[fg], t[bg])
            if c < 4.5:
                failures.append(f"{name}/{mode}: {fg} on {bg} = {c:.2f}")
    for fg in ["ok", "warn", "bad", "accent"]:  # pills: text on a 14% tint of itself
        pill = mix(t[fg], t["surface"], 0.14)
        if (c := contrast(t[fg], pill)) < 4.5:
            failures.append(f"{name}/{mode}: {fg} pill = {c:.2f}")
    if (c := contrast(t["on-accent"], t["accent"])) < 4.5:
        failures.append(f"{name}/{mode}: on-accent = {c:.2f}")
    if (c := contrast(t["accent"], t["surface"])) < 3:
        failures.append(f"{name}/{mode}: focus ring = {c:.2f}")
    return failures


def main():
    failures = []
    out = []
    keys = list(NEUTRALS) + ["accent", "on-accent"]
    for name in ACCENTS:
        light, dark = theme(name, "light"), theme(name, "dark")
        failures += check(name, "light", light) + check(name, "dark", dark)
        sel = (":root, " if name == "blue" else "") + f'[data-accent="{name}"]'
        body = " ".join(f"--{k}: light-dark({light[k]}, {dark[k]});" for k in keys)
        out.append(f"{sel} {{ {body} }}")
    print("\n".join(out))
    for f in failures:
        print("FAIL", f, file=sys.stderr)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
