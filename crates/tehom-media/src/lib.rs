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
                "Yeterli disk alanı yok: {} MiB gerekli, {} MiB kullanılabilir. Alan açıp yeniden dene.",
                required_bytes.div_ceil(1024 * 1024),
                available / (1024 * 1024)
            ),
        ));
    }
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
            "Yalnızca normal video dosyaları açılabilir",
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
                "format=duration:stream=width,height,codec_name",
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
        .ok_or_else(|| io::Error::other("Video süresi okunamadı"))?;
    let stream = &json["streams"][0];
    let dimension = |name: &str| {
        stream[name]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| io::Error::other("Video görüntüsü bulunamadı"))
    };
    Ok(VideoInfo {
        duration: seconds,
        width: dimension("width")?,
        height: dimension("height")?,
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
    render(input, output, start, end, cancelled, None)
}

pub fn export_automated(
    input: &Path,
    output: &Path,
    start: f64,
    end: f64,
    cancelled: &AtomicBool,
    identity: &str,
) -> io::Result<()> {
    if identity.is_empty()
        || identity.len() > 128
        || !identity
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Geçersiz dışa aktarma kimliği",
        ));
    }
    render(input, output, start, end, cancelled, Some(identity))
}

pub fn export_identity(path: &Path) -> io::Result<Option<String>> {
    let path = path.canonicalize()?;
    if !path.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Normal bir video dosyası seç",
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

fn render(
    input: &Path,
    output: &Path,
    start: f64,
    end: f64,
    cancelled: &AtomicBool,
    identity: Option<&str>,
) -> io::Result<()> {
    let total = inspect_cancellable(input, cancelled)?.duration;
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start || end > total {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Geçersiz klip aralığı",
        ));
    }
    if output.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Hedef dosya zaten var",
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
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Klip iptal edildi",
        ));
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
        .arg(input.canonicalize()?)
        .args([
            "-t",
            &(end - start).to_string(),
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-movflags",
            "+faststart",
        ]);
    if let Some(identity) = identity {
        command.args([
            "-vf",
            "scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,setsar=1",
            "-metadata",
            &format!("comment=tehom:{identity}"),
        ]);
    }
    run(command.arg(temporary.path()), cancelled, None)?;
    inspect_cancellable(temporary.path(), cancelled)?;
    temporary.as_file().sync_all()?;
    if cancelled.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Klip iptal edildi",
        ));
    }
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    Ok(())
}

fn run(
    command: &mut Command,
    cancelled: &AtomicBool,
    timeout: Option<Duration>,
) -> io::Result<Vec<u8>> {
    if cancelled.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "İşlem iptal edildi",
        ));
    }
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()?;
    let started = Instant::now();
    let status = (|| {
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "İşlem iptal edildi",
                ));
            }
            if timeout.is_some_and(|limit| started.elapsed() >= limit) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Video aracı zamanında yanıt vermedi",
                ));
            }
            if stdout.metadata()?.len() > 65536 || stderr.metadata()?.len() > 65536 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Video aracı çok fazla hata verisi üretti",
                ));
            }
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
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
