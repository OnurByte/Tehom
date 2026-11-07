<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%" alt="Tehom. Paste a link. Get clips as MP4 files. The AI you choose picks the moments worth keeping; nothing is ever posted for you.">
</picture>

**Version 0.1** ⁜ [Download](https://github.com/OnurByte/Tehom/releases/latest) for Linux, macOS and Windows ⁜ verified end to end on Linux with real YouTube videos ([evidence](docs/evidence/byok-live.md)) ⁜ [what is next](docs/status.md)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/readme/install-dark.svg">
  <img src="assets/readme/install-light.svg" width="100%" alt="Install with one command on macOS, Linux and Windows">
</picture>

```sh
curl -fsSL https://raw.githubusercontent.com/OnurByte/Tehom/main/install.sh | sh
```

```powershell
irm https://raw.githubusercontent.com/OnurByte/Tehom/main/install.ps1 | iex
```

The installer puts Tehom in your user folder without admin rights, checks the download's SHA-256 and adds Tehom to your app menu. If FFmpeg, yt-dlp or GStreamer are missing, it shows the command and asks before running it. Release files are checksum-verified but not yet code-signed.

**Pick your AI on first launch.** A short guide asks once:

| Option | What you need |
| --- | --- |
| Your API key | OpenAI, Anthropic, Gemini or OpenRouter. The key lives in your system keyring. |
| Your subscription | A signed-in [Codex](https://github.com/openai/codex) or [Claude Code](https://claude.com/claude-code) CLI. No key needed. |
| On this computer | Ollama, llama.cpp, LM Studio or vLLM. Nothing leaves your machine. |

Only captions are sent to the AI you allow; videos and clips stay on your computer. Press **Ctrl+K** (Cmd+K on macOS) anywhere to find any action.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/readme/steps-dark.svg">
  <img src="assets/readme/steps-light.svg" width="100%" alt="How it works: add a link, let it choose, review, save">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/readme/features-dark.svg">
  <img src="assets/readme/features-light.svg" width="100%" alt="What it does: vertical, horizontal or original clips; watches channels; your choice of AI; uses captions; private by default; Kick videos (experimental).">
</picture>

---

## For power users

<details>
<summary><b>Command line cheat sheet</b></summary>

| Command | Does |
| --- | --- |
| `tehom ai --provider codex --consent` | Use Codex, Claude Code (`claude-code`), `openai`, `anthropic`, `gemini`, `openrouter` or `local` |
| `tehom ai-key anthropic` | Save an API key in the system keyring (read from stdin, never argv) |
| `tehom add URL --mode auto-draft` | Add a channel, video or Kick VOD |
| `tehom import-video FILE` | Queue a local file |
| `tehom check` / `tehom run-once` | Look for new videos / process the queue |
| `tehom retry ID` / `tehom cancel ID` | Control one job |
| `tehom status` / `tehom events --video ID` | Read state (works while the daemon runs) |
| `tehom revise-clip ID --start-ms … --end-ms …` | Save a new draft version |
| `tehom export-profile vertical\|horizontal\|original` | Clip shape |
| `tehom save-original ID ~/Videos` | Keep the source video |
| `tehom export-transcript ID edit.srt` | Edit captions, then import them back |
| `tehom import-transcript ID file.tr.srt` | Add a caption version (language from the name) |
| `tehom cleanup --dry-run` | Free space used by downloads |

Modes: `manual`, `auto-draft`, `auto-export`, `paused`. Use `--data DIR` for another data folder.

</details>

<details>
<summary><b>Run in the background (Linux)</b></summary>

```sh
tehom service install --config ~/.config/tehom/tehom.toml --enable
tehom service uninstall --disable
```

Copy [`config/tehom.example.toml`](config/tehom.example.toml) first. Every key is either applied or rejected with a reason. See the [user-service guide](docs/headless-service.md).

</details>

<details>
<summary><b>Safety and limits</b></summary>

- The engine only contacts `www.youtube.com` and your loopback model; social and publishing APIs are refused in code. yt-dlp downloads the videos.
- Writes are atomic and never replace existing files; partial renders are never published. [Disk and model details](docs/disk-and-models.md).
- No web app, SaaS, accounts, social upload, scheduler or AppImage.

</details>

<details id="development">
<summary><b>Development</b></summary>

Needs Rust 1.99.1, FFmpeg/ffprobe, yt-dlp and GStreamer with base, good and libav plug-ins. The [CI workflow](.github/workflows/native-prototype.yml) lists exact packages per platform.

```sh
cargo run -p tehom-desktop
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
packaging/release-archive.sh       # the archive the installers download
packaging/linux/build-deb.sh       # optional unsigned .deb
```

Pushing a `v*` tag runs the [release workflow](.github/workflows/release.yml), which builds the archives for Linux (x86_64, arm64), macOS (Apple Silicon, Intel) and Windows. Card images follow the onur.af brand standard and are generated by [`assets/readme/generate.py`](assets/readme/generate.py) in light and dark variants.

</details>

**Licence:** AGPL-3.0-or-later. Geist fonts under the included OFL. The brand logo is the original supplied design; see [asset provenance](assets/README.md).
