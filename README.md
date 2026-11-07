<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/icon-dark.svg">
  <img src="assets/brand/icon.svg" width="64" alt="Tehom">
</picture>

# Tehom

**Paste a video or channel link. Get short clips as MP4 files, made on your own computer.**

Tehom is a native Rust desktop app (GPUI Kit) and CLI. It downloads a video, reads its captions, asks a **local** AI model for the best moments and renders them with FFmpeg. Nothing is uploaded or published anywhere.

> **Status:** under active development, not a released product. Every card below says what is working, experimental or still missing. Full detail: [requirement status](docs/status.md) · [specification](docs/master-spec.txt).

**Legend:** ✅ working and tested · 🧪 experimental · ⏳ not built yet

---

## How it works

<table>
<tr>
<td width="25%" valign="top">

### 1 · Add
Paste a **YouTube channel**, a **single video** or pick a **local file**.

</td>
<td width="25%" valign="top">

### 2 · Analyse
Captions are read and sent to **your local model** (Ollama or OpenAI-compatible on `127.0.0.1`).

</td>
<td width="25%" valign="top">

### 3 · Review
Up to five 20–90 s drafts. Play, trim and save new versions.

</td>
<td width="25%" valign="top">

### 4 · Save
MP4 files land in your folder, manually or automatically.

</td>
</tr>
</table>

---

## Features

<table>
<tr>
<td width="33%" valign="top">

#### 📺 Sources ✅
- YouTube channels via RSS (new videos only)
- Single YouTube videos, shorts, live links
- Local video files, read in place
- Duplicate videos are never processed twice

</td>
<td width="33%" valign="top">

#### 🤖 Local AI ✅
- Ollama or OpenAI-compatible server
- Loopback only; no cloud, no API keys
- Every suggested range is validated
- Invalid output produces no clip

</td>
<td width="33%" valign="top">

#### 🎬 Export ✅
- Vertical 1080×1920, horizontal 1920×1080 or original size
- Never overwrites an existing file
- Partial renders are never published
- Live progress while rendering

</td>
</tr>
<tr>
<td valign="top">

#### ⚙️ Automation ✅
- Modes: Manual, Auto Draft, Auto Export, Paused
- Background daemon and systemd user service
- Survives restarts and crashes
- Cancelled jobs never restart by themselves

</td>
<td valign="top">

#### 📝 Transcripts ✅
- Platform captions (Turkish preferred)
- Upload SRT/VTT with a language tag
- Export → edit → re-import as a new version
- No automatic speech recognition

</td>
<td valign="top">

#### 🗂️ Your files ✅
- Save the original video separately
- Missing or moved files are flagged, not hidden
- Optional cleanup of downloads (off by default)
- Full event history of every job

</td>
</tr>
<tr>
<td valign="top">

#### 🧪 Experimental
- Kick VOD links (`kick.com/video/<id>`)
- Debian package build script (unsigned)

</td>
<td valign="top">

#### ⏳ Not built yet
- Speech recognition (manual, opt-in)
- Remote AI with your own key
- Captions on clips, drag reframing
- Light theme, parallel jobs

</td>
<td valign="top">

#### ⏳ Release gates
- Signed installers (deb, Flatpak, dmg, exe)
- Updates and rollback
- macOS/Windows end-to-end runs
- Security and licence audit

</td>
</tr>
</table>

---

## Quick start

<table>
<tr>
<td width="50%" valign="top">

#### 🖥️ Desktop

```sh
cargo run -p tehom-desktop
```

1. **Settings** → choose your local model and clip folder
2. **Home** → paste a channel or video link, pick a mode
3. **Clips** → review, trim, save

</td>
<td width="50%" valign="top">

#### ⌨️ Command line

```sh
tehom ai --model your-installed-model
tehom add https://www.youtube.com/@channel --mode auto-export
tehom run-once
tehom status
```

</td>
</tr>
</table>

**You need:** Rust 1.99.1, FFmpeg/ffprobe, yt-dlp, GStreamer with base, good and libav plug-ins, and a local model server. On Windows/macOS use the official [GStreamer installers](https://gstreamer.freedesktop.org/download/); the [CI workflow](.github/workflows/native-prototype.yml) lists the exact Linux packages.

---

## CLI cheat sheet

<table>
<tr>
<td width="50%" valign="top">

#### Sources and jobs
| Command | Does |
| --- | --- |
| `tehom add URL --mode auto-draft` | Channel, video or Kick VOD |
| `tehom import-video FILE` | Queue a local file |
| `tehom check` / `run-once` | Look for new videos / process queue |
| `tehom retry ID` / `cancel ID` | Control one job |
| `tehom status` / `events --video ID` | Read state (works while daemon runs) |

</td>
<td width="50%" valign="top">

#### Clips and files
| Command | Does |
| --- | --- |
| `tehom revise-clip ID --start-ms … --end-ms …` | New draft version |
| `tehom export-profile vertical\|horizontal\|original` | Clip shape |
| `tehom save-original ID ~/Videos` | Keep the source video |
| `tehom export-transcript ID edit.srt` | Edit captions |
| `tehom import-transcript ID file.srt --language tr` | Add a caption version |
| `tehom cleanup --dry-run` | Free downloaded sources |

</td>
</tr>
<tr>
<td valign="top">

#### Background service (Linux)
```sh
tehom service install --config ~/.config/tehom/tehom.toml --enable
tehom service uninstall --disable
```
See [user-service guide](docs/headless-service.md).

</td>
<td valign="top">

#### Configuration
Copy [`config/tehom.example.toml`](config/tehom.example.toml). Every key is either applied or rejected with a reason; unknown keys are errors. CLI `--data` and `--interval-minutes` override the file.

</td>
</tr>
</table>

---

## Safety

<table>
<tr>
<td width="33%" valign="top">

#### 🔒 Network
The engine only talks to `www.youtube.com` and your loopback model. Social and publishing APIs are refused in code.

</td>
<td width="33%" valign="top">

#### 💾 Files
Writes are atomic and never replace existing files. Disk space is checked before downloads and renders. [Details](docs/disk-and-models.md).

</td>
<td width="33%" valign="top">

#### 🚫 Out of scope
No web shell, SaaS, accounts, social upload, scheduler or AppImage.

</td>
</tr>
</table>

---

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
packaging/linux/build-deb.sh   # optional: unsigned .deb in target/deb/
```

Tests render real media with FFmpeg. Preview tests need the GStreamer plug-ins above and fail without them. A passing build is not proof of playback or packaging; see [evidence](docs/evidence/).

**Licence:** AGPL-3.0-or-later. Geist fonts under the included OFL. The brand geometry is the user's original; see [asset provenance](assets/README.md).
