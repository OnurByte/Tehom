use std::{
    io::{self, Read, Seek},
    path::Path,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub codec: String,
}

/// Check the destination volume without creating or deleting user files.
pub fn require_space(path: &Path, required_bytes: u64) -> io::Result<()> {
    let path = path.canonicalize()?;
    let available = fs2::available_space(path)?;
    if available < required_bytes {
        return Err(io::Error::new(
            io::ErrorKind::StorageFull,
            format!(
                "Not enough disk space: {} MiB needed, {} MiB available. Free some space and retry.",
                required_bytes.div_ceil(1024 * 1024),
                available / (1024 * 1024)
            ),
        ));
    }
    Ok(())
}

/// Copies `source` to `destination` through a temporary file in the
/// destination folder, publishing only a complete copy and never replacing
/// an existing file.
pub fn copy_noclobber(source: &Path, destination: &Path) -> io::Result<()> {
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "The destination file already exists",
        ));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let size = std::fs::metadata(source)?.len();
    require_space(parent, size.saturating_add(64 * 1024 * 1024))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".tehom-")
        .suffix(".part")
        .tempfile_in(parent)?;
    let copied = io::copy(&mut std::fs::File::open(source)?, temporary.as_file_mut())?;
    if copied != size {
        return Err(io::Error::other("The copy is incomplete"));
    }
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

pub fn duration(path: &Path) -> io::Result<f64> {
    Ok(inspect(path)?.duration)
}

pub fn inspect(path: &Path) -> io::Result<VideoInfo> {
    inspect_cancellable(path, &AtomicBool::new(false))
}

fn inspect_cancellable(path: &Path, cancelled: &AtomicBool) -> io::Result<VideoInfo> {
    let path = path.canonicalize()?;
    if !path.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Only regular video files can be opened",
        ));
    }
    let output = run(
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-select_streams",
                "v:0",
                "-show_entries",
                "format=duration:stream=width,height,codec_name:stream_side_data=rotation:stream_tags=rotate",
                "-of",
                "json",
            ])
            .arg(path),
        cancelled,
        Some(Duration::from_secs(30)),
    )?;
    let json: serde_json::Value = serde_json::from_slice(&output)?;
    let seconds = json["format"]["duration"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| io::Error::other("The video length could not be read"))?;
    let stream = &json["streams"][0];
    let dimension = |name: &str| {
        stream[name]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| io::Error::other("The file has no video picture"))
    };
    // Report display geometry: FFmpeg auto-rotates on decode, so a
    // quarter-turn rotation swaps the stored width and height.
    let rotation = stream["side_data_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|data| data["rotation"].as_f64())
        .or_else(|| stream["tags"]["rotate"].as_str()?.parse::<f64>().ok())
        .unwrap_or(0.0);
    let quarter_turn = (rotation.round() as i64).rem_euclid(180) == 90;
    let (width, height) = (dimension("width")?, dimension("height")?);
    let (width, height) = if quarter_turn {
        (height, width)
    } else {
        (width, height)
    };
    Ok(VideoInfo {
        duration: seconds,
        width,
        height,
        codec: stream["codec_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned(),
    })
}

pub fn export(input: &Path, output: &Path, start: f64, end: f64) -> io::Result<()> {
    export_cancellable(input, output, start, end, &AtomicBool::new(false))
}

pub fn export_cancellable(
    input: &Path,
    output: &Path,
    start: f64,
    end: f64,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    render(input, output, start, end, cancelled, None, None, None)
}

/// Frame geometry for an automatic export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// Scale to cover `width`×`height`, then centre-crop.
    Fill { width: u32, height: u32 },
    /// Keep the source geometry, trimmed to even dimensions.
    Source,
}
impl Framing {
    fn filter(self) -> String {
        match self {
            Self::Fill { width, height } => format!(
                "scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height},setsar=1"
            ),
            Self::Source => "scale=trunc(iw/2)*2:trunc(ih/2)*2,setsar=1".into(),
        }
    }
}

/// Vertical 1080×1920 automatic export.
pub fn export_automated(
    input: &Path,
    output: &Path,
    start: f64,
    end: f64,
    cancelled: &AtomicBool,
    identity: &str,
) -> io::Result<()> {
    export_framed(
        input,
        output,
        start,
        end,
        cancelled,
        identity,
        Framing::Fill {
            width: 1080,
            height: 1920,
        },
    )
}

/// Automatic export with an ownership identity and the given framing.
pub fn export_framed(
    input: &Path,
    output: &Path,
    start: f64,
    end: f64,
    cancelled: &AtomicBool,
    identity: &str,
    framing: Framing,
) -> io::Result<()> {
    export_framed_observed(
        input, output, start, end, cancelled, identity, framing, None,
    )
}

/// `export_framed` reporting render progress (0.0–1.0) to `progress`.
#[allow(clippy::too_many_arguments)]
pub fn export_framed_observed(
    input: &Path,
    output: &Path,
    start: f64,
    end: f64,
    cancelled: &AtomicBool,
    identity: &str,
    framing: Framing,
    progress: Option<&dyn Fn(f64)>,
) -> io::Result<()> {
    export_overlaid(
        input,
        output,
        start,
        end,
        cancelled,
        identity,
        framing,
        progress,
        &Overlay::default(),
    )
}

/// A short sound mixed in at the start of a clip, synthesized by FFmpeg.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sound {
    #[default]
    None,
    Whoosh,
    Pop,
}
impl Sound {
    /// The lavfi source and the shaping filters applied to it.
    fn synth(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::None => None,
            Self::Whoosh => Some((
                "anoisesrc=d=0.6:c=pink:r=48000:a=0.5",
                "highpass=f=400,lowpass=f=5000,afade=t=in:d=0.3:curve=exp,afade=t=out:st=0.3:d=0.3",
            )),
            Self::Pop => Some((
                "sine=f=880:d=0.15:r=48000",
                "afade=t=in:d=0.005,afade=t=out:st=0.01:d=0.14,volume=0.7",
            )),
        }
    }
}

/// Extra layers for a framed export: an ASS caption script burned in after
/// framing, and an intro sound mixed under the original audio.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    pub subtitles: Option<String>,
    pub sound: Sound,
}

/// `export_framed_observed` with burned-in captions and an intro sound.
#[allow(clippy::too_many_arguments)]
pub fn export_overlaid(
    input: &Path,
    output: &Path,
    start: f64,
    end: f64,
    cancelled: &AtomicBool,
    identity: &str,
    framing: Framing,
    progress: Option<&dyn Fn(f64)>,
    overlay: &Overlay,
) -> io::Result<()> {
    if identity.is_empty()
        || identity.len() > 128
        || !identity
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid export identity",
        ));
    }
    render(
        input,
        output,
        start,
        end,
        cancelled,
        Some((identity, framing)),
        progress,
        Some(overlay),
    )
}

/// Where captions sit on the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    Top,
    Middle,
    #[default]
    Bottom,
}

/// Caption appearance; colors are `#RRGGBB`.
#[derive(Debug, Clone, Copy)]
pub struct CaptionLook<'a> {
    pub color: &'a str,
    pub outline: &'a str,
    /// Text size relative to the default (1.0).
    pub scale: f64,
    pub placement: Placement,
    pub uppercase: bool,
}

/// `#RRGGBB` as an opaque ASS `&HAABBGGRR` color.
fn ass_color(hex: &str) -> io::Result<String> {
    let digits = hex
        .strip_prefix('#')
        .filter(|d| d.len() == 6 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Caption color must look like #FFFFFF, not {hex:?}"),
            )
        })?
        .to_ascii_uppercase();
    Ok(format!(
        "&H00{}{}{}",
        &digits[4..6],
        &digits[2..4],
        &digits[0..2]
    ))
}

/// Caption text with everything ASS would interpret neutralized: braces
/// start override tags and backslashes start escapes, so neither survives.
fn ass_text(text: &str, uppercase: bool) -> String {
    let text = if uppercase {
        text.to_uppercase()
    } else {
        text.to_owned()
    };
    let mut escaped = String::new();
    for c in text.trim().chars() {
        match c {
            '{' => escaped.push('('),
            '}' => escaped.push(')'),
            '\\' => escaped.push('/'),
            '\n' => escaped.push_str("\\N"),
            c if c.is_control() => escaped.push(' '),
            c => escaped.push(c),
        }
    }
    escaped
}

/// Builds an ASS script for a `width`×`height` output: bold sans with a thick
/// outline, kept clear of the areas short-video apps cover with buttons.
pub fn ass_subtitles(
    cues: &[(u64, u64, &str)],
    look: &CaptionLook,
    width: u32,
    height: u32,
) -> io::Result<String> {
    let (primary, outline) = (ass_color(look.color)?, ass_color(look.outline)?);
    let (w, h) = (f64::from(width), f64::from(height));
    let font = (w.min(h) * 0.075 * look.scale.clamp(0.5, 2.0))
        .round()
        .max(8.0);
    let border = (font * 0.09).round().max(2.0);
    let tall = height > width;
    let (alignment, margin) = match look.placement {
        Placement::Top => (8, h * if tall { 0.14 } else { 0.07 }),
        Placement::Middle => (5, 0.0),
        Placement::Bottom => (2, h * if tall { 0.20 } else { 0.07 }),
    };
    let side = (w * 0.08).round();
    let clock = |ms: u64| {
        format!(
            "{}:{:02}:{:02}.{:02}",
            ms / 3_600_000,
            ms / 60_000 % 60,
            ms / 1000 % 60,
            ms % 1000 / 10
        )
    };
    let mut script = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {width}\nPlayResY: {height}\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n\
         [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Default,Arial,{font},{primary},{primary},{outline},&H80000000,-1,0,0,0,100,100,0,0,1,{border},{shadow},{alignment},{side},{side},{margin},1\n\n\
         [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        shadow = (border / 2.0).round(),
        margin = margin.round(),
    );
    for &(start, end, text) in cues {
        let text = ass_text(text, look.uppercase);
        if end > start && !text.trim().is_empty() {
            script.push_str(&format!(
                "Dialogue: 0,{},{},Default,,0,0,0,,{text}\n",
                clock(start),
                clock(end)
            ));
        }
    }
    Ok(script)
}

/// Escapes a path for an FFmpeg filter option inside a filtergraph: first
/// the option level (`\ ' :`), then the graph level (`\ ' [ ] , ;`).
fn filter_path(path: &Path) -> String {
    let mut path = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        path = path.replace('\\', "/");
    }
    let escape = |text: &str, special: &[char]| {
        let mut escaped = String::new();
        for c in text.chars() {
            if special.contains(&c) {
                escaped.push('\\');
            }
            escaped.push(c);
        }
        escaped
    };
    escape(
        &escape(&path, &['\\', '\'', ':']),
        &['\\', '\'', '[', ']', ',', ';'],
    )
}

/// Fails with a clear message when this FFmpeg cannot draw captions.
fn require_libass(cancelled: &AtomicBool) -> io::Result<()> {
    let help = run(
        Command::new("ffmpeg").args(["-hide_banner", "-v", "error", "-h", "filter=ass"]),
        cancelled,
        Some(Duration::from_secs(30)),
    )?;
    if String::from_utf8_lossy(&help).contains("Filter ass") {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "This FFmpeg cannot draw captions because it was built without libass. Install an FFmpeg build with libass, or turn captions off.",
        ))
    }
}

/// Whether the file has at least one audio stream.
pub fn has_audio(path: &Path) -> io::Result<bool> {
    has_audio_cancellable(path, &AtomicBool::new(false))
}

fn has_audio_cancellable(path: &Path, cancelled: &AtomicBool) -> io::Result<bool> {
    let output = run(
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=index",
                "-of",
                "csv=p=0",
            ])
            .arg(path.canonicalize()?),
        cancelled,
        Some(Duration::from_secs(30)),
    )?;
    Ok(!String::from_utf8_lossy(&output).trim().is_empty())
}

/// Writes the first audio track as 16 kHz mono 16-bit WAV for speech
/// recognition. `output` is overwritten; callers pass a private path.
pub fn extract_speech(input: &Path, output: &Path, cancelled: &AtomicBool) -> io::Result<()> {
    if !has_audio_cancellable(input, cancelled)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "This video has no sound to transcribe",
        ));
    }
    run(
        Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-protocol_whitelist",
                "file,pipe",
                "-i",
            ])
            .arg(input.canonicalize()?)
            .args([
                "-vn",
                "-map",
                "0:a:0",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(output),
        cancelled,
        None,
    )?;
    Ok(())
}

/// Writes a JPEG cover (the frame 0.5 s in, or the first frame of clips
/// shorter than a second) to a new temporary file, removed when dropped.
pub fn cover_frame(input: &Path, cancelled: &AtomicBool) -> io::Result<tempfile::NamedTempFile> {
    let info = inspect_cancellable(input, cancelled)?;
    let at = if info.duration >= 1.0 { "0.5" } else { "0" };
    let cover = tempfile::Builder::new()
        .prefix("tehom-cover-")
        .suffix(".jpg")
        .tempfile()?;
    run(
        Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-protocol_whitelist",
                "file,pipe",
                "-ss",
                at,
                "-i",
            ])
            .arg(input.canonicalize()?)
            .args(["-frames:v", "1", "-q:v", "2", "-f", "image2"])
            .arg(cover.path()),
        cancelled,
        Some(Duration::from_secs(60)),
    )?;
    if cover.as_file().metadata()?.len() == 0 {
        return Err(io::Error::other(
            "No cover picture could be read from the video",
        ));
    }
    Ok(cover)
}

pub fn export_identity(path: &Path) -> io::Result<Option<String>> {
    let path = path.canonicalize()?;
    if !path.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose a regular video file",
        ));
    }
    let bytes = run(
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-show_entries",
                "format_tags=comment",
                "-of",
                "json",
            ])
            .arg(path),
        &AtomicBool::new(false),
        Some(Duration::from_secs(30)),
    )?;
    let data: serde_json::Value = serde_json::from_slice(&bytes)?;
    Ok(data["format"]["tags"]["comment"]
        .as_str()
        .and_then(|value| value.strip_prefix("tehom:"))
        .map(str::to_owned))
}

#[allow(clippy::too_many_arguments)]
fn render(
    input: &Path,
    output: &Path,
    start: f64,
    end: f64,
    cancelled: &AtomicBool,
    identity: Option<(&str, Framing)>,
    progress: Option<&dyn Fn(f64)>,
    overlay: Option<&Overlay>,
) -> io::Result<()> {
    let total = inspect_cancellable(input, cancelled)?.duration;
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start || end > total {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid clip range",
        ));
    }
    if output.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "The destination file already exists",
        ));
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // Reserve space for a 16 Mbit/s encode plus headroom; this is a preflight, not a size guarantee.
    let required = ((end - start) * 2_000_000.0).ceil() as u64;
    require_space(parent, required.saturating_add(256 * 1024 * 1024))?;
    let temporary = tempfile::Builder::new()
        .prefix(".tehom-")
        .suffix(".mp4")
        .tempfile_in(parent)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Clip cancelled"));
    }
    let sound = overlay.map_or(Sound::None, |overlay| overlay.sound);
    // Kept alive until FFmpeg has finished reading it.
    let mut script = None;
    if let Some(text) = overlay.and_then(|overlay| overlay.subtitles.as_deref()) {
        use std::io::Write;
        require_libass(cancelled)?;
        let mut file = tempfile::Builder::new()
            .prefix(".tehom-")
            .suffix(".ass")
            .tempfile_in(parent)?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
        script = Some(file);
    }
    let mut command = Command::new("ffmpeg");
    command
        .args([
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-protocol_whitelist",
            "file,pipe",
            "-ss",
            &start.to_string(),
            "-i",
        ])
        .arg(input.canonicalize()?);
    if let Some((source, _)) = sound.synth() {
        command.args(["-f", "lavfi", "-i", source]);
    }
    command.args([
        "-t",
        &(end - start).to_string(),
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-movflags",
        "+faststart",
    ]);
    let mut video = identity.map(|(_, framing)| framing.filter());
    if let Some(script) = &script {
        // Drawn after scaling and cropping, so captions match the output frame.
        let captions = format!("ass=filename={}", filter_path(script.path()));
        video = Some(match video {
            Some(framing) => format!("{framing},{captions}"),
            None => captions,
        });
    }
    match sound.synth() {
        None => {
            command.args(["-map", "0:v:0", "-map", "0:a?"]);
            if let Some(video) = video {
                command.args(["-vf", &video]);
            }
        }
        Some((_, shape)) => {
            let stereo = "aformat=sample_rates=48000:channel_layouts=stereo";
            let audio = if has_audio_cancellable(input, cancelled)? {
                format!(
                    "[0:a:0]{stereo}[main];[1:a]{shape},{stereo}[fx];[main][fx]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[a]"
                )
            } else {
                // No source audio: the effect, then silence for the clip's length.
                format!("[1:a]{shape},{stereo},apad[a]")
            };
            let video = video.unwrap_or_else(|| "null".into());
            command.args([
                "-filter_complex",
                &format!("[0:v:0]{video}[v];{audio}"),
                "-map",
                "[v]",
                "-map",
                "[a]",
            ]);
        }
    }
    if let Some((identity, _)) = identity {
        command.args(["-metadata", &format!("comment=tehom:{identity}")]);
    }
    match progress {
        None => {
            run(command.arg(temporary.path()), cancelled, None)?;
        }
        Some(report) => {
            // A private file keeps progress out of the bounded stdout capture.
            let mut log = tempfile::Builder::new()
                .prefix("tehom-progress-")
                .tempfile()?;
            command
                .arg("-progress")
                .arg(log.path())
                .args(["-stats_period", "0.5"]);
            let length = end - start;
            let mut last = 0.0;
            let mut tick = || {
                if let Some(seconds) = latest_progress(log.as_file_mut()) {
                    let fraction = (seconds / length).clamp(0.0, 1.0);
                    if fraction > last {
                        last = fraction;
                        report(fraction);
                    }
                }
            };
            run_observed(command.arg(temporary.path()), cancelled, None, &mut tick)?;
            report(1.0);
        }
    }
    inspect_cancellable(temporary.path(), cancelled)?;
    if sound != Sound::None && !has_audio_cancellable(temporary.path(), cancelled)? {
        return Err(io::Error::other("The rendered clip has no sound"));
    }
    temporary.as_file().sync_all()?;
    if cancelled.load(Ordering::Acquire) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Clip cancelled"));
    }
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    Ok(())
}

/// Seconds rendered so far, from the tail of an FFmpeg `-progress` log.
fn latest_progress(log: &mut std::fs::File) -> Option<f64> {
    use std::io::Seek;
    let length = log.metadata().ok()?.len();
    log.seek(io::SeekFrom::Start(length.saturating_sub(2048)))
        .ok()?;
    let mut tail = String::new();
    log.take(2048).read_to_string(&mut tail).ok()?;
    tail.lines()
        .rev()
        .find_map(|line| line.strip_prefix("out_time_us="))
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|micros| *micros >= 0)
        .map(|micros| micros as f64 / 1_000_000.0)
}

/// Explain a failed launch in the user's words: the platform text ("program not found") names
/// neither the program nor the fix.
fn spawn_error(program: &std::ffi::OsStr, error: io::Error) -> io::Error {
    let name = Path::new(program)
        .file_stem()
        .unwrap_or(program)
        .to_string_lossy()
        .into_owned();
    match error.kind() {
        io::ErrorKind::NotFound => {
            let what = if matches!(name.as_str(), "ffmpeg" | "ffprobe") {
                "FFmpeg (ffmpeg and ffprobe)"
            } else {
                name.as_str()
            };
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{name} was not found. Install {what} and make sure it is on PATH, then reopen Tehom."
                ),
            )
        }
        io::ErrorKind::PermissionDenied => io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{name} could not run: permission denied. Check the file permissions."),
        ),
        _ => io::Error::new(error.kind(), format!("{name} could not start: {error}")),
    }
}

fn run(
    command: &mut Command,
    cancelled: &AtomicBool,
    timeout: Option<Duration>,
) -> io::Result<Vec<u8>> {
    run_observed(command, cancelled, timeout, &mut || {})
}

/// Like `run`, calling `tick` on every poll while the process runs.
fn run_observed(
    command: &mut Command,
    cancelled: &AtomicBool,
    timeout: Option<Duration>,
    tick: &mut dyn FnMut(),
) -> io::Result<Vec<u8>> {
    if cancelled.load(Ordering::Acquire) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled"));
    }
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()
        .map_err(|error| spawn_error(command.get_program(), error))?;
    let started = Instant::now();
    let status = (|| {
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled"));
            }
            if timeout.is_some_and(|limit| started.elapsed() >= limit) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "The video tool did not respond in time",
                ));
            }
            if stdout.metadata()?.len() > 65536 || stderr.metadata()?.len() > 65536 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The video tool produced too much error output",
                ));
            }
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            tick();
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            if let Err(kill_error) = child.kill() {
                eprintln!("Media shutdown: {kill_error}");
            }
            if let Err(wait_error) = child.wait() {
                eprintln!("Media shutdown: {wait_error}");
            }
            return Err(error);
        }
    };
    if !status.success() {
        stderr.rewind()?;
        let mut message = String::new();
        stderr.take(65536).read_to_string(&mut message)?;
        return Err(io::Error::other(format!("{status}: {message}")));
    }
    stdout.rewind()?;
    let mut bytes = Vec::new();
    stdout.take(65536).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_export_and_reject_overwrite() {
        let dir = std::env::temp_dir().join(format!("tehom-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("source.mp4");
        let output = dir.join("clip.mp4");
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=160x90:rate=10",
                    "-t",
                    "2",
                    "-c:v",
                    "libx264"
                ])
                .arg(&input)
                .status()
                .unwrap()
                .success()
        );
        assert!(export(&input, &output, 1.0, 0.0).is_err());
        export(&input, &output, 0.25, 1.25).unwrap();
        assert!((duration(&output).unwrap() - 1.0).abs() < 0.2);
        let bytes = std::fs::read(&output).unwrap();
        assert!(export(&input, &output, 0.0, 1.0).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), bytes);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod atomic_tests {
    use super::*;
    #[test]
    fn output_is_not_visible_until_complete() {
        let directory = std::env::temp_dir().join(format!("tehom-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let input = directory.join("source.mp4");
        let output = directory.join("clip.mp4");
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=640x360:rate=30",
                    "-t",
                    "3",
                    "-c:v",
                    "libx264",
                    "-preset",
                    "ultrafast"
                ])
                .arg(&input)
                .status()
                .unwrap()
                .success()
        );
        let worker_output = output.clone();
        let worker = std::thread::spawn(move || export(&input, &worker_output, 0.0, 2.5));
        let mut exposed_empty_file = false;
        while !worker.is_finished() {
            if let Ok(metadata) = output.metadata() {
                exposed_empty_file |= metadata.len() == 0;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        worker.join().unwrap().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
        assert!(
            !exposed_empty_file,
            "unfinished output was visible as an MP4"
        );
    }
}

#[cfg(feature = "preview")]
pub mod preview;

#[cfg(test)]
mod missing_program_tests {
    use super::*;

    #[test]
    fn a_missing_program_is_named_with_the_fix_not_the_platform_text() {
        let error = run(
            &mut Command::new("tehom-no-such-program"),
            &AtomicBool::new(false),
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        let message = error.to_string();
        assert!(
            message.contains("tehom-no-such-program was not found"),
            "{message}"
        );
        assert!(message.contains("PATH"), "{message}");
        assert!(!message.contains("program not found"), "{message}");
    }
}

#[cfg(test)]
mod caption_tests {
    use super::*;

    #[test]
    fn ass_script_honours_style_and_cannot_be_injected() {
        let look = CaptionLook {
            color: "#ffcc00",
            outline: "#000000",
            scale: 1.0,
            placement: Placement::Top,
            uppercase: true,
        };
        let script = ass_subtitles(
            &[
                (0, 1500, "hi {\\b0\\pos(1,1)} back\\slash\nnext"),
                (3_723_450, 3_724_000, "late"),
                (5, 5, "empty range"),
                (6, 9, "   "),
            ],
            &look,
            1080,
            1920,
        )
        .unwrap();
        assert!(script.contains("PlayResX: 1080\nPlayResY: 1920"));
        // #ffcc00 is BGR in ASS; top alignment is 8 with a safe top margin.
        assert!(
            script.contains("Style: Default,Arial,81,&H0000CCFF,&H0000CCFF,&H00000000,"),
            "{script}"
        );
        assert!(script.contains(",8,86,86,269,1\n"), "{script}");
        let lines: Vec<&str> = script
            .lines()
            .filter(|l| l.starts_with("Dialogue:"))
            .collect();
        assert_eq!(lines.len(), 2, "{script}");
        assert_eq!(
            lines[0],
            "Dialogue: 0,0:00:00.00,0:00:01.50,Default,,0,0,0,,HI (/B0/POS(1,1)) BACK/SLASH\\NNEXT"
        );
        assert!(lines[1].starts_with("Dialogue: 0,1:02:03.45,1:02:04.00,"));
        let text = lines[0].rsplit(",,").next().unwrap();
        assert!(!text.contains('{') && !text.contains('}'));
        assert_eq!(text.matches('\\').count(), 1, "only our own line break");
        let bottom = ass_subtitles(
            &[],
            &CaptionLook {
                placement: Placement::Bottom,
                uppercase: false,
                scale: 1.25,
                ..look
            },
            1080,
            1920,
        )
        .unwrap();
        assert!(bottom.contains(",101,") && bottom.contains(",2,86,86,384,1\n"));
        for bad in ["fff", "#12345", "#12345G", "red", "#1234567"] {
            assert!(
                ass_subtitles(&[], &CaptionLook { color: bad, ..look }, 10, 10).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn filter_paths_are_escaped_for_option_and_graph_levels() {
        assert_eq!(
            filter_path(Path::new("/tmp/it's a:b,[c];d.ass")),
            "/tmp/it\\\\\\'s a\\\\:b\\,\\[c\\]\\;d.ass"
        );
        if cfg!(windows) {
            assert_eq!(filter_path(Path::new(r"C:\x\a.ass")), r"C\\:/x/a.ass");
        }
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;

    #[test]
    fn probe_timeout_stops_and_reaps_the_process() {
        let started = Instant::now();
        let error = run(
            Command::new("sleep").arg("5"),
            &AtomicBool::new(false),
            Some(Duration::from_millis(20)),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
