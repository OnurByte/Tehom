# Live run: YouTube to AI to MP4 (2026-11-07)

Host: Arch Linux x86_64, yt-dlp 2026.08.19, FFmpeg 9.0.2, GStreamer with base, good and libav plug-ins, Codex CLI 0.167.0 signed in with ChatGPT. Built from branch `feature/byok`.

## Auto Export

- Input: `https://www.youtube.com/watch?v=qD0_yWgifDM` (TED-Ed, "The science of spiciness", 3 min 55 s).
- Setup: `tehom ai --provider codex --model "" --consent`, then `tehom run-once`.
- Download: the video and its English and Turkish captions came from YouTube.
- Analysis: Codex ran in a read-only sandbox in an empty folder, with the captions on stdin and a JSON schema for the answer. It returned five non-overlapping clips. The engine validated them, saved them as drafts and exported each one.
- Time: the whole job took 68 s and finished `complete`.

| Range | Title chosen by the model |
| --- | --- |
| 19.6–71.2 s | Baharatlılık nasıl hissedilir? |
| 71.2–100.8 s | Biber ağzı, wasabi burnu neden yakar? |
| 101.2–134.7 s | Acılık Scoville ölçeğiyle nasıl ölçülür? |
| 139.2–175.5 s | İnsanlar neden acı yemeye başladı? |
| 175.6–213.1 s | Acı yemek neden hoşumuza gider? |

ffprobe check on all five MP4s: H.264, 1080 × 1920, and durations matching their ranges (51.6, 29.6, 33.5, 36.3 and 37.5 s). A frame from the second clip shows the source animation, centre-cropped to vertical.

## Auto Draft

- Input: `https://www.youtube.com/watch?v=Xu-QfE_1ksk` ("The science of laughter").
- Result: five 20–90 s drafts and no files, as the mode intends.
- Title: the record took the real title from yt-dlp ("The science of laughter - Sasha Winkler").

## Problems found and fixed in this run

- Asking for subtitles with `tr.*,en.*` also matched about 70 machine-translated tracks. YouTube answered HTTP 429 and the whole download failed. Tehom now requests exact languages only (`tr`, `en` and their `-orig` and regional variants).
- A failed download now quotes yt-dlp's own `ERROR:` line in the job message.
- Leaving the model empty for Codex now uses the user's Codex default instead of stopping the job.

## Not covered by this run

- The HTTPS API providers (OpenAI, Anthropic, Gemini, OpenRouter) were exercised only against loopback fixtures with each provider's real request and response shapes. No live API key was available on the host.
- Claude Code was not signed in on the host, so it was tested with a fixture script only.
- Clip quality was judged by reading the titles and ranges. It was not compared with human picks.
