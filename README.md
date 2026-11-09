<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%" alt="Tehom. Paste a link. Get vertical clips.">
</picture>

<p align="center">
  <a href="https://github.com/OnurByte/Tehom/releases/latest"><b>Download</b></a> ⁜ Linux, macOS and Windows ⁜ Free and open source ⁜ Built with <a href="https://gpui-kit.com/">GPUI Kit</a>
</p>

## Install

### <img src="assets/readme/os-apple.svg" height="20" alt=""> <img src="assets/readme/os-linux.svg" height="20" alt=""> macOS and Linux

Open **Terminal** and paste:

```sh
curl -fsSL https://raw.githubusercontent.com/OnurByte/Tehom/main/install.sh | sh
```

### <img src="assets/readme/os-windows.svg" height="20" alt=""> Windows

Open **PowerShell** and paste:

```powershell
irm https://raw.githubusercontent.com/OnurByte/Tehom/main/install.ps1 | iex
```

Then open **Tehom** from your apps. A short guide helps you choose an AI the first time.

**Updating:** Tehom tells you when a new version is out. Choose **Install update** in Settings, or run `tehom update`.

## How it works

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/readme/steps-dark.svg">
  <img src="assets/readme/steps-light.svg" width="100%" alt="Add a link, let the AI choose, review, save">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/readme/features-dark.svg">
  <img src="assets/readme/features-light.svg" width="100%" alt="What Tehom does">
</picture>

## Choose your AI

| Option | You need |
| --- | --- |
| API key | An OpenAI, Anthropic, Gemini or OpenRouter key |
| Subscription | Codex or Claude Code, signed in |
| This computer | Ollama, LM Studio, llama.cpp or vLLM |

Only captions go to the AI you choose. Your videos and clips stay on your computer.

Press **Ctrl+K** (**Cmd+K** on Mac) to find any action.

---

<details>
<summary><b>Command line</b></summary>

```sh
tehom ai --provider codex --consent          # or openai, anthropic, gemini, openrouter, claude-code, local
tehom ai-key anthropic                       # saves a key in your system keyring
tehom add https://www.youtube.com/@channel --mode auto-export
tehom run-once                               # check and process now
tehom status                                 # what is happening
tehom export-profile vertical                # or horizontal, original
tehom update                                 # install the latest version (--rollback undoes it)
tehom info                                   # version, folders and AI in use
tehom service install --config tehom.toml --enable   # keep running in the background (Linux)
```

Run `tehom --help` for every command. Settings file: [`config/tehom.example.toml`](config/tehom.example.toml).

</details>

<details>
<summary><b>Privacy and safety</b></summary>

- Tehom posts nothing unless you connect an Instagram account and choose to publish. Otherwise it only talks to the video site and the AI you allow.
- Instagram publishing uses Instagram's private mobile protocol, not an official API. Instagram may change it or restrict accounts that use it; use an account you can afford to lose access to.
- Speech is transcribed on this computer with whisper.cpp when a video has no captions.
- API keys live in your system keyring, never in Tehom's files.
- Existing files are never overwritten, and half-made clips never appear.

More: [security](docs/security.md), [disk and models](docs/disk-and-models.md).

</details>

<details>
<summary><b>Build from source</b></summary>

A native Rust app built with [GPUI Kit](https://gpui-kit.com/). Needs Rust 1.99.1, FFmpeg, yt-dlp and GStreamer (base, good and libav plug-ins).

```sh
cargo run -p tehom-desktop
cargo test --workspace
```

Status and evidence: [docs/status.md](docs/status.md).

</details>

AGPL-3.0-or-later ⁜ Geist fonts under the OFL ⁜ [Asset provenance](assets/README.md)
