# Disk space, models and drafts

Before downloading, Tehom requires 1 GiB available on the media volume (16 MiB for captions alone). During download it checks once per second and cancels if less than 256 MiB remains (16 MiB for captions). MP4 preflight estimates 16 Mbit/s for the selected duration plus 256 MiB headroom on the export volume. These are conservative checks, not reservations or exact size guarantees; competing writes can still exhaust space. Existing output files are never replaced. Free space and explicitly retry a job that needs input.

The platform space query uses [fs2 available_space](https://docs.rs/fs2/0.4.3/fs2/fn.available_space.html). Download cleanup is opt-in (`[storage] keep_downloads = false`, `tehom cleanup`); disk quotas are still pending.

Local model servers must support JSON-schema structured outputs (Ollama `format` or OpenAI-compatible `response_format`). Responses still pass independent timestamp/duration validation; an empty or invalid selection produces no clip.

Open a library clip with **Taslağı düzenle**, adjust start/end in seconds and choose **Yeni taslak kaydet** to persist a new 20–90 second draft. Previous drafts and published files remain intact. Identical ranges reuse an existing record. Saving a revision does not queue automatic rendering; **MP4 kaydet** exports the current editor selection.
