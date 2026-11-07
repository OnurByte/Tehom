# Regenerates the README card SVGs in both system themes:
#   python3 assets/readme/generate.py assets/readme
#
# Follows the onur.af brand standard (brand.md, docs/design.md): monochrome
# tokens, Helvetica Neue, normal-weight headings with size-based hierarchy,
# 12 px card corners, the shared corner-lit 1 px edge, no glow, no accent
# colour, no animation, no em dash or middle dot (the separator is ⁜).
# The Tehom mark is embedded from assets/brand/icon.svg with its viewBox,
# path data and evenodd fill rule unchanged; only the icon material is applied.
import re
import sys
from pathlib import Path

out = Path(sys.argv[1])
source = (Path(__file__).resolve().parents[1] / "brand" / "icon.svg").read_text()
MARK_PATH = re.search(r' d="([^"]+)"', source).group(1)
assert 'fill-rule="evenodd"' in source and 'viewBox="0 0 512 512"' in source

THEMES = {
    "dark": dict(background="#000000", text="#ffffff", muted="#ffffff", border="#404040",
                 surface="#222222", face=("#ffffff", "#a0a0a0"), shadow=("#606060", 0.95)),
    "light": dict(background="#ffffff", text="#0d0d0d", muted="#333333", border="#b3b3b3",
                  surface="#f2f2f2", face=("#606060", "#0d0d0d"), shadow=("#000000", 0.8)),
}
SANS = 'font-family="&quot;Helvetica Neue&quot;, Helvetica, Arial, sans-serif"'
MONO = 'font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"'
WIDTH = 960


def mix(a, b, weight):
    """color-mix(in srgb, a weight, b)."""
    pa = [int(a[i:i + 2], 16) for i in (1, 3, 5)]
    pb = [int(b[i:i + 2], 16) for i in (1, 3, 5)]
    return "#" + "".join(f"{round(x * weight + y * (1 - weight)):02x}" for x, y in zip(pa, pb))


def esc(text):
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


class Canvas:
    def __init__(self, theme):
        self.t = THEMES[theme]
        self.parts = []
        light = mix(self.t["text"], self.t["border"], 0.55)
        # Corner lights: ellipses spanning half of each element's own size.
        self.defs = [
            f'<radialGradient id="tr" cx="1" cy="0" r="0.5" gradientUnits="objectBoundingBox">'
            f'<stop offset="0" stop-color="{light}"/><stop offset="1" stop-color="{light}" stop-opacity="0"/></radialGradient>',
            f'<radialGradient id="bl" cx="0" cy="1" r="0.5" gradientUnits="objectBoundingBox">'
            f'<stop offset="0" stop-color="{light}"/><stop offset="1" stop-color="{light}" stop-opacity="0"/></radialGradient>',
            f'<linearGradient id="face" x1="0" y1="0" x2="1" y2="1">'
            f'<stop offset="0" stop-color="{self.t["face"][0]}"/><stop offset="1" stop-color="{self.t["face"][1]}"/></linearGradient>',
        ]

    def card(self, x, y, w, h, fill=None):
        fill = fill or self.t["background"]
        box = f'x="{x + 0.5}" y="{y + 0.5}" width="{w - 1}" height="{h - 1}" rx="12"'
        self.parts += [
            f'<rect {box} fill="{fill}" stroke="{self.t["border"]}"/>',
            f'<rect {box} fill="none" stroke="url(#tr)"/>',
            f'<rect {box} fill="none" stroke="url(#bl)"/>',
        ]

    def text(self, x, y, content, size=16, weight=400, muted=False, mono=False, anchor="start"):
        colour = self.t["muted"] if muted else self.t["text"]
        family = MONO if mono else SANS
        self.parts.append(
            f'<text x="{x}" y="{y}" font-size="{size}" font-weight="{weight}" fill="{colour}" '
            f'text-anchor="{anchor}" {family}>{esc(content)}</text>')

    def pill(self, x, y, label):
        width = 20 + len(label) * 7.4
        self.parts.append(
            f'<rect x="{x - width + 0.5}" y="{y + 0.5}" width="{width - 1}" height="23" rx="8" '
            f'fill="none" stroke="{self.t["border"]}"/>')
        self.text(x - width / 2, y + 16, label, size=13, muted=True, anchor="middle")

    def mark(self, x, y, size):
        s = size
        colour, alpha = self.t["shadow"]
        self.defs.append(
            f'<filter id="depth" x="-10%" y="-10%" width="130%" height="130%">'
            f'<feDropShadow dx="{0.02 * s:.2f}" dy="{0.04 * s:.2f}" stdDeviation="{0.005 * s:.2f}" '
            f'flood-color="{colour}" flood-opacity="{alpha}"/></filter>')
        self.parts.append(
            f'<svg x="{x}" y="{y}" width="{s}" height="{s}" viewBox="0 0 512 512" aria-hidden="true">'
            f'<path fill="url(#face)" fill-rule="evenodd" filter="url(#depth)" d="{MARK_PATH}"/></svg>')

    def save(self, name, theme, height, label):
        body = "\n".join(self.parts)
        defs = "".join(self.defs)
        (out / f"{name}-{theme}.svg").write_text(
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" '
            f'viewBox="0 0 {WIDTH} {height}" role="img" aria-label="{esc(label)}">\n'
            f"<defs>{defs}</defs>\n{body}\n</svg>\n")


def wrap(c, x, y, lines, size=14, gap=22, muted=True):
    for i, line in enumerate(lines):
        c.text(x, y + i * gap, line, size=size, muted=muted)


for theme in THEMES:
    # Hero: signature, statement, short description.
    c = Canvas(theme)
    c.card(0, 0, WIDTH, 248)
    c.mark(40, 40, 56)
    c.text(112, 79, "Tehom", size=32)
    c.text(40, 158, "Paste a link. Get clips as MP4 files.", size=36)
    wrap(c, 40, 198, ["A local AI model picks the moments worth keeping. Everything runs",
                      "on your computer, and nothing is posted anywhere."], size=16, gap=26)
    c.save("hero", theme, 248, "Tehom. Paste a link. Get clips as MP4 files.")

    # Steps: one section, four ordered columns.
    c = Canvas(theme)
    c.card(0, 0, WIDTH, 216)
    c.text(32, 52, "How it works", size=18)
    steps = [
        ("Add a link", ["A YouTube channel, one", "video or a local file."]),
        ("Let it choose", ["Your local model reads", "the captions and picks."]),
        ("Review", ["Watch each draft and", "adjust start or end."]),
        ("Save", ["MP4 files land in your", "folder, by hand or auto."]),
    ]
    column = (WIDTH - 64 - 3 * 32) / 4
    for i, (title, lines) in enumerate(steps):
        x = 32 + i * (column + 32)
        c.text(x, 104, f"0{i + 1}", size=14, muted=True, mono=True)
        c.text(x, 132, title, size=16, weight=500)
        wrap(c, x, 160, lines)
    c.save("steps", theme, 216, "How it works: add a link, let it choose, review, save")

    # Features: section heading, 2 × 3 grid of bordered items with plain status.
    c = Canvas(theme)
    features = [
        ("Vertical, horizontal or original", ["1080 × 1920 for Shorts and Reels,", "1920 × 1080 or the source size."], "Works"),
        ("Watches channels", ["New videos on a channel become", "drafts or MP4 files on their own."], "Works"),
        ("Stays on your computer", ["Only the video site and your local", "model are contacted. No posting."], "Works"),
        ("Uses captions", ["Platform captions, or SRT and VTT", "files you add or correct."], "Works"),
        ("Keeps your files", ["Existing files are never replaced;", "unfinished clips never appear."], "Works"),
        ("Kick videos", ["Paste a kick.com video link.", "Downloading is still being tested."], "Experimental"),
    ]
    c.text(0, 26, "What it does", size=18)
    item_w, item_h, gap = (WIDTH - 2 * 24) / 3, 152, 24
    for i, (title, lines, status) in enumerate(features):
        x, y = (i % 3) * (item_w + gap), 48 + (i // 3) * (item_h + gap)
        c.card(x, y, item_w, item_h)
        c.pill(x + item_w - 20, y + 20, status)
        c.text(x + 20, y + 76, title, size=16, weight=500)
        wrap(c, x + 20, y + 104, lines)
    planned = "Planned ⁜ speech to text on request ⁜ your own AI key ⁜ captions on clips ⁜ reframing"
    c.text(0, 48 + 2 * item_h + gap + 40, planned, size=14, muted=True)
    height = 48 + 2 * item_h + gap + 56
    c.save("features", theme, height, "What it does, and what is planned")

    # Install: two machine-data blocks.
    c = Canvas(theme)
    c.text(0, 26, "Install with one command", size=18)
    blocks = [
        ("macOS ⁜ Linux", ["curl -fsSL https://raw.githubusercontent.com/", "OnurByte/Tehom/main/install.sh | sh"]),
        ("Windows PowerShell", ["irm https://raw.githubusercontent.com/", "OnurByte/Tehom/main/install.ps1 | iex"]),
    ]
    block_w = (WIDTH - 24) / 2
    for i, (label, lines) in enumerate(blocks):
        x = i * (block_w + 24)
        c.card(x, 48, block_w, 132, fill=THEMES[theme]["surface"])
        c.text(x + 20, 80, label, size=14, muted=True)
        for j, line in enumerate(lines):
            c.text(x + 20, 120 + j * 26, line, size=15, mono=True)
    c.save("install", theme, 180, "Install with one command on macOS, Linux and Windows")
