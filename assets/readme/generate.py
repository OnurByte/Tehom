# Regenerates the README card SVGs: python3 assets/readme/generate.py assets/readme
# hero.svg is hand-written; the brand logo is intentionally not used here.
import sys
out = sys.argv[1]
FONT = 'font-family="-apple-system, &quot;Segoe UI&quot;, Helvetica, Arial, sans-serif"'

ICONS = {
    "link": '<g fill="none" stroke="{c}" stroke-width="4" stroke-linecap="round"><rect x="2" y="12" width="24" height="16" rx="8"/><rect x="18" y="12" width="24" height="16" rx="8"/></g>',
    "spark": '<path d="M22 2 C25 16 30 19 42 22 C30 25 25 28 22 42 C19 28 14 25 2 22 C14 19 19 16 22 2 Z" fill="{c}"/>',
    "scissors": '<g fill="none" stroke="{c}" stroke-width="4" stroke-linecap="round"><circle cx="10" cy="32" r="7"/><circle cx="34" cy="32" r="7"/><path d="M15 27 L36 4 M29 27 L8 4"/></g>',
    "save": '<g fill="none" stroke="{c}" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"><path d="M22 4 V28 M12 18 L22 28 L32 18"/><path d="M4 30 V38 H40 V30"/></g>',
    "clock": '<g fill="none" stroke="{c}" stroke-width="4" stroke-linecap="round"><circle cx="22" cy="22" r="18"/><path d="M22 11 V22 L30 27"/></g>',
    "shield": '<g fill="none" stroke="{c}" stroke-width="4" stroke-linejoin="round" stroke-linecap="round"><path d="M22 3 L39 9 V21 C39 31 32 38 22 41 C12 38 5 31 5 21 V9 Z"/><path d="M14 22 L20 28 L31 16"/></g>',
    "text": '<g fill="none" stroke="{c}" stroke-width="4" stroke-linecap="round"><rect x="3" y="6" width="38" height="32" rx="7"/><path d="M11 18 H33 M11 27 H25"/></g>',
    "folder": '<g fill="none" stroke="{c}" stroke-width="4" stroke-linejoin="round"><path d="M3 10 H17 L21 15 H41 V37 H3 Z"/></g>',
    "phone": '<g fill="none" stroke="{c}" stroke-width="4" stroke-linejoin="round"><rect x="11" y="2" width="22" height="40" rx="5"/><path d="M19 22 L27 27 L19 32 Z" fill="{c}" stroke="none"/></g>',
}

def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

def card(x, y, w, h, color, icon, title, lines, badge=None, number=None):
    parts = [f'<g transform="translate({x} {y})">',
             f'<rect width="{w}" height="{h}" rx="22" fill="#262626" stroke="#393939"/>',
             f'<rect x="22" y="0.5" width="{w-44}" height="5" rx="2.5" fill="{color}"/>',
             f'<rect x="24" y="30" width="64" height="64" rx="18" fill="{color}" opacity=".16"/>',
             f'<g transform="translate(34 40)">{ICONS[icon].format(c=color)}</g>']
    if number is not None:
        parts.append(f'<text x="{w-28}" y="62" font-size="40" font-weight="800" fill="#393939" text-anchor="end" {FONT}>{number}</text>')
    parts.append(f'<text x="24" y="128" font-size="23" font-weight="700" fill="#f2f4f8" {FONT}>{esc(title)}</text>')
    for i, line in enumerate(lines):
        parts.append(f'<text x="24" y="{160 + i*26}" font-size="17" fill="#a8a8b3" {FONT}>{esc(line)}</text>')
    if badge:
        label, bcolor = badge
        bw = 16 + len(label) * 8.6
        parts.append(f'<rect x="{w-24-bw}" y="38" width="{bw}" height="28" rx="14" fill="{bcolor}" opacity=".18"/>')
        parts.append(f'<text x="{w-24-bw/2}" y="57" font-size="14" font-weight="700" fill="{bcolor}" text-anchor="middle" {FONT}>{esc(label)}</text>')
    parts.append('</g>')
    return "\n".join(parts)

def svg(name, w, h, label, body):
    open(f"{out}/{name}.svg", "w").write(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" role="img" aria-label="{esc(label)}">\n'
        f'<rect width="{w}" height="{h}" rx="28" fill="#161616"/>\n{body}\n</svg>\n')

P, B, C, K, G, Y = "#be95ff", "#78a9ff", "#33b1ff", "#ff7eb6", "#42be65", "#f1c21b"

steps = [
    (P, "link", "Paste a link", ["A YouTube channel, a video", "or a file on your computer."]),
    (B, "spark", "AI finds moments", ["Your own local model reads", "the captions and picks."]),
    (C, "scissors", "Check and trim", ["Watch each draft, move the", "start or end if you like."]),
    (K, "save", "Save as MP4", ["Clips land in your folder,", "by hand or automatically."]),
]
body = "\n".join(card(24 + i * 294, 24, 276, 230, c, ic, t, l, number=i + 1) for i, (c, ic, t, l) in enumerate(steps))
svg("steps", 1200, 278, "Four steps: paste a link, AI finds moments, check and trim, save as MP4", body)

OK = ("WORKS", G)
EXP = ("EXPERIMENTAL", Y)
SOON = ("COMING", "#a8a8b3")
features = [
    (P, "phone", "Any shape", ["Vertical for Shorts/Reels,", "horizontal, or original size."], OK),
    (B, "clock", "Runs on its own", ["Watch a channel; new videos", "become clips while you sleep."], OK),
    (G, "shield", "Private by design", ["Talks only to the video site and", "your computer. Never posts."], OK),
    (C, "text", "Caption friendly", ["Uses video captions; bring", "or fix your own SRT files."], OK),
    (K, "folder", "Your files are safe", ["Nothing is ever overwritten;", "half-made clips never appear."], OK),
    (Y, "link", "Kick VODs", ["Paste a kick.com video link.", "Still being tested."], EXP),
]
body = "\n".join(card(24 + (i % 3) * 392, 24 + (i // 3) * 254, 368, 230, c, ic, t, l, badge=b) for i, (c, ic, t, l, b) in enumerate(features))
svg("features", 1200, 532, "Features: any clip shape, automatic channel watching, private by design, caption friendly, safe files, experimental Kick VODs", body)

soon = [
    ("#a8a8b3", "text", "Speech to text", ["For videos without captions.", "Only when you ask for it."], SOON),
    ("#a8a8b3", "spark", "Bring your own AI key", ["Optional cloud models, with", "your explicit permission."], SOON),
    ("#a8a8b3", "scissors", "Captions on clips", ["Burned-in subtitles and", "drag-to-reframe."], SOON),
]
body = "\n".join(card(24 + i * 392, 24, 368, 230, c, ic, t, l, badge=b) for i, (c, ic, t, l, b) in enumerate(soon))
svg("coming", 1200, 278, "Coming later: speech to text, bring your own AI key, captions on clips", body)

def terminal(x, y, w, title, color, lines):
    parts = [f'<g transform="translate({x} {y})">',
             f'<rect width="{w}" height="150" rx="20" fill="#0b0b0f" stroke="#393939"/>',
             f'<circle cx="28" cy="26" r="6" fill="#ee5396"/><circle cx="48" cy="26" r="6" fill="#f1c21b"/><circle cx="68" cy="26" r="6" fill="#42be65"/>',
             f'<text x="{w-24}" y="31" font-size="16" font-weight="700" fill="{color}" text-anchor="end" {FONT}>{esc(title)}</text>']
    for i, (prompt, cmd) in enumerate(lines):
        parts.append(f'<text x="28" y="{82 + i*30}" font-size="17" fill="#f2f4f8" font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"><tspan fill="{color}">{esc(prompt)}</tspan> {esc(cmd)}</text>')
    parts.append('</g>')
    return "\n".join(parts)

body = (terminal(24, 24, 564, "macOS · Linux", P, [("$", "curl -fsSL https://raw.githubusercontent.com/"), (" ", "OnurByte/Tehom/main/install.sh | sh")]) + "\n" +
        terminal(612, 24, 564, "Windows", B, [("PS>", "irm https://raw.githubusercontent.com/"), ("   ", "OnurByte/Tehom/main/install.ps1 | iex")]))
svg("install", 1200, 198, "One command to install: curl install.sh | sh on macOS and Linux, irm install.ps1 | iex on Windows", body)
