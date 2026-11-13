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

ICON_DIR = Path(__file__).resolve().parents[1] / "icons"
# Brand glyphs, path data unchanged from assets/icons (Simple Icons, CC0).
GLYPHS = {n: re.search(r' d="([^"]+)"', (ICON_DIR / f"{n}.svg").read_text()).group(1)
          for n in ("instagram", "tiktok", "youtubeshorts", "bluesky", "facebook")}

THEMES = {
    "dark": dict(background="#000000", text="#ffffff", muted="#ffffff", border="#404040",
                 surface="#222222", face=("#ffffff", "#a0a0a0"), shadow=("#606060", 0.95)),
    "light": dict(background="#ffffff", text="#0d0d0d", muted="#333333", border="#b3b3b3",
                  surface="#f2f2f2", face=("#606060", "#0d0d0d"), shadow=("#000000", 0.8)),
}
SANS = 'font-family="&quot;Helvetica Neue&quot;, Helvetica, Arial, sans-serif"'
MONO = 'font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"'
WIDTH = 960

# Line pictograms in a 32 x 32 grid, drawn with the shared icon material.
ICONS = {
    "link": '<rect x="3" y="11" width="15" height="10" rx="5"/><rect x="14" y="11" width="15" height="10" rx="5"/>',
    "spark": '<path d="M14 4 C15.5 11 18 13.5 25 15 C18 16.5 15.5 19 14 26 C12.5 19 10 16.5 3 15 C10 13.5 12.5 11 14 4 Z"/><path d="M25 3 V9 M22 6 H28"/>',
    "review": '<rect x="3" y="5" width="26" height="18" rx="4"/><path d="M13 10 L19 14 L13 18 Z"/><path d="M3 28 H29"/><path d="M10 26 V30"/>',
    "save": '<path d="M16 4 V20 M10 14 L16 20 L22 14"/><path d="M5 21 V27 H27 V21"/>',
    "shape": '<rect x="4" y="3" width="11" height="20" rx="2.5"/><rect x="18" y="13" width="11" height="15" rx="2.5"/>',
    "watch": '<circle cx="16" cy="16" r="12"/><path d="M16 9 V16 L21 19"/>',
    "local": '<path d="M4 15 L16 5 L28 15"/><path d="M7 13 V27 H25 V13"/><path d="M13 27 V19 H19 V27"/>',
    "captions": '<rect x="3" y="6" width="26" height="20" rx="4"/><path d="M8 15 H24 M8 20 H18"/>',
    "files": '<path d="M4 9 V26 H28 V12 H15 L12 9 Z"/><path d="M11 19 L14.5 22.5 L21 16"/>',
    "lab": '<path d="M12 4 H20 M13.5 4 V12 L6 25 C5 27 6 28 8 28 H24 C26 28 27 27 26 25 L18.5 12 V4"/><path d="M9 21 H23"/>',
}


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
            f'<linearGradient id="iface" x1="0" y1="0" x2="32" y2="32" gradientUnits="userSpaceOnUse">'
            f'<stop offset="0" stop-color="{self.t["face"][0]}"/><stop offset="1" stop-color="{self.t["face"][1]}"/></linearGradient>',
            f'<linearGradient id="iface24" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">'
            f'<stop offset="0" stop-color="{self.t["face"][0]}"/><stop offset="1" stop-color="{self.t["face"][1]}"/></linearGradient>',
            f'<filter id="idepth" x="-20%" y="-20%" width="150%" height="150%">'
            f'<feDropShadow dx="0.64" dy="1.28" stdDeviation="0.16" flood-color="{self.t["shadow"][0]}" '
            f'flood-opacity="{self.t["shadow"][1]}"/></filter>',
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

    def icon(self, x, y, name, size=32):
        self.parts.append(
            f'<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="0 0 32 32" aria-hidden="true">'
            f'<g fill="none" stroke="url(#iface)" stroke-width="2.4" stroke-linecap="round" '
            f'stroke-linejoin="round" filter="url(#idepth)">{ICONS[name]}</g></svg>')

    def glyph(self, x, y, name, size=32):
        self.parts.append(
            f'<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="0 0 24 24" aria-hidden="true">'
            f'<path fill="url(#iface24)" d="{GLYPHS[name]}"/></svg>')

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
    # Centred: the signature (mark plus name) and both lines share one axis.
    c = Canvas(theme)
    c.card(0, 0, WIDTH, 260)
    mark, gap, name_width = 48, 14, 88  # "Tehom" at 32 px is about 88 px wide
    left = (WIDTH - (mark + gap + name_width)) / 2
    c.mark(left, 44, mark)
    c.text(left + mark + gap, 80, "Tehom", size=32)
    c.text(WIDTH / 2, 166, "Paste a link. Get vertical clips.", size=36, anchor="middle")
    c.text(WIDTH / 2, 210, "Transcribed on your computer, captioned, cut to 9:16. No cloud, no subscription.",
           size=16, muted=True, anchor="middle")
    c.save("hero", theme, 260, "Tehom. Paste a link. Get vertical clips.")

    # Steps: one section, four ordered columns.
    c = Canvas(theme)
    c.card(0, 0, WIDTH, 236)
    c.text(32, 52, "How it works", size=18)
    steps = [
        ("link", "Add a link", ["A channel, a video, an RSS", "feed or a local file."]),
        ("spark", "Let it choose", ["Speech is written here, your", "AI picks the best moments."]),
        ("review", "Edit", ["Fix the captions, colours", "and timing in Studio."]),
        ("save", "Save or post", ["MP4 files, or Reels on", "Instagram, by hand or auto."]),
    ]
    column = (WIDTH - 64 - 3 * 32) / 4
    for i, (icon, title, lines) in enumerate(steps):
        x = 32 + i * (column + 32)
        c.icon(x, 84, icon)
        c.text(x + column - 8, 104, f"0{i + 1}", size=14, muted=True, mono=True, anchor="end")
        c.text(x, 152, title, size=16, weight=500)
        wrap(c, x, 180, lines)
    c.save("steps", theme, 236, "How it works: add a link, let it choose, edit, save or post")

    # Features: section heading, 2 × 3 grid of bordered items with plain status.
    c = Canvas(theme)
    features = [
        ("shape", "Vertical, horizontal or original", ["1080 × 1920 for Shorts and Reels,", "1920 × 1080 or the source size."], "Works"),
        ("watch", "Watches sources", ["Channels and RSS feeds turn new", "videos into clips on their own."], "Works"),
        ("spark", "Your AI, your choice", ["OpenAI, Anthropic, Gemini, OpenRouter,", "Codex, Claude Code or a local model."], "Works"),
        ("captions", "Captions you can edit", ["Written on this computer, burned in,", "with colour, size and sound."], "Works"),
        ("local", "Private by default", ["Videos stay here; only captions go to", "the AI you allow. You choose what posts."], "Works"),
        ("lab", "Post to five platforms", ["Instagram, TikTok, YouTube Shorts,", "Bluesky and Facebook Reels."], "Experimental"),
    ]
    c.text(0, 26, "What it does", size=18)
    item_w, item_h, gap = (WIDTH - 2 * 24) / 3, 152, 24
    for i, (icon, title, lines, status) in enumerate(features):
        x, y = (i % 3) * (item_w + gap), 48 + (i // 3) * (item_h + gap)
        c.card(x, y, item_w, item_h)
        c.icon(x + 20, y + 20, icon)
        c.pill(x + item_w - 20, y + 24, status)
        c.text(x + 20, y + 76, title, size=16, weight=500)
        wrap(c, x + 20, y + 104, lines)
    planned = "Also ⁜ Light and dark mode ⁜ Ctrl+K command palette ⁜ never overwrites files ⁜ Soon ⁜ LinkedIn ⁜ Snapchat ⁜ Bilibili ⁜ Kwai ⁜ Likee"
    c.text(0, 48 + 2 * item_h + gap + 40, planned, size=14, muted=True)
    height = 48 + 2 * item_h + gap + 56
    c.save("features", theme, height, "What it does")

    # Posting: one card per platform with its glyph and a plain status.
    c = Canvas(theme)
    c.text(0, 26, "Post where your audience is", size=18)
    platforms = [
        ("instagram", "Instagram Reels", ["Sign in once in the app.", "Post by hand or automatically."], "Experimental"),
        ("tiktok", "TikTok", ["Sign in once in a browser window.", "Post by hand or automatically."], "Experimental"),
        ("youtubeshorts", "YouTube Shorts", ["Sign in once in a browser window.", "Post by hand or automatically."], "Experimental"),
        ("bluesky", "Bluesky", ["Handle and app password;", "Bluesky's official API."], "Experimental"),
        ("facebook", "Facebook Reels", ["Page id and token from your own", "Meta app; official Graph API."], "Experimental"),
    ]
    item_w, item_h, gap = (WIDTH - 2 * 24) / 3, 152, 24
    for i, (glyph, title, lines, status) in enumerate(platforms):
        x, y = (i % 3) * (item_w + gap), 48 + (i // 3) * (item_h + gap)
        c.card(x, y, item_w, item_h)
        c.glyph(x + 20, y + 20, glyph)
        c.pill(x + item_w - 20, y + 24, status)
        c.text(x + 20, y + 84, title, size=16, weight=500)
        wrap(c, x + 20, y + 110, lines)
    c.save("post", theme, 48 + 2 * item_h + gap + 8, "Post to Instagram Reels, TikTok, YouTube Shorts, Bluesky and Facebook Reels")
