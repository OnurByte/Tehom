use std::process::Command;
use std::sync::atomic::AtomicBool;
use tehom_media::{CaptionLook, Framing, Overlay, Placement, Sound};

fn source(path: &std::path::Path, audio: bool) {
    let mut command = Command::new("ffmpeg");
    command.args([
        "-nostdin",
        "-v",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=320x180:r=10",
    ]);
    if audio {
        command.args(["-f", "lavfi", "-i", "sine=f=220:r=48000"]);
    }
    command.args(["-t", "3", "-c:v", "libx264", "-pix_fmt", "yuv420p"]);
    if audio {
        command.args(["-c:a", "aac"]);
    }
    assert!(command.arg(path).status().unwrap().success());
}

/// Brightest luma byte of the frame at 0.5 s, downscaled to gray.
fn brightest(path: &std::path::Path) -> u8 {
    let output = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-ss", "0.5", "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=108:192",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            "-",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout.into_iter().max().unwrap()
}

#[test]
fn captions_and_intro_sounds_render_valid_vertical_clips() {
    // Quote, colon and comma in the folder name exercise filter escaping.
    // Windows forbids ':' in names; its drive letter (`C:`) covers the colon there.
    let root = tempfile::tempdir().unwrap();
    let name = if cfg!(windows) {
        "it's a,b c"
    } else {
        "it's a:b,c"
    };
    let directory = root.path().join(name);
    std::fs::create_dir(&directory).unwrap();
    let look = CaptionLook {
        color: "#FFFFFF",
        outline: "#000000",
        scale: 1.0,
        placement: Placement::Bottom,
        uppercase: false,
    };
    let script =
        tehom_media::ass_subtitles(&[(0, 1500, "Hello {world}")], &look, 1080, 1920).unwrap();
    for audio in [true, false] {
        let input = directory.join(format!("source-{audio}.mp4"));
        source(&input, audio);
        assert_eq!(tehom_media::has_audio(&input).unwrap(), audio);
        for sound in [Sound::None, Sound::Whoosh, Sound::Pop] {
            let output = directory.join(format!("clip-{audio}-{sound:?}.mp4"));
            tehom_media::export_overlaid(
                &input,
                &output,
                0.5,
                2.5,
                &AtomicBool::new(false),
                "captioned-clip",
                Framing::Fill {
                    width: 1080,
                    height: 1920,
                },
                None,
                &Overlay {
                    subtitles: Some(script.clone()),
                    sound,
                },
            )
            .unwrap_or_else(|error| panic!("{audio} {sound:?}: {error}"));
            let info = tehom_media::inspect(&output).unwrap();
            assert_eq!((info.width, info.height), (1080, 1920));
            assert!((info.duration - 2.0).abs() < 0.2, "{}", info.duration);
            assert_eq!(
                tehom_media::has_audio(&output).unwrap(),
                audio || sound != Sound::None,
                "{audio} {sound:?}"
            );
            assert_eq!(
                tehom_media::export_identity(&output).unwrap().as_deref(),
                Some("captioned-clip")
            );
            // White captions on a black source prove libass drew them.
            assert!(brightest(&output) > 200, "{audio} {sound:?}");
        }
    }
    // Without captions the same black frame stays dark.
    let plain = directory.join("plain.mp4");
    tehom_media::export_overlaid(
        &directory.join("source-true.mp4"),
        &plain,
        0.5,
        2.5,
        &AtomicBool::new(false),
        "plain-clip",
        Framing::Fill {
            width: 1080,
            height: 1920,
        },
        None,
        &Overlay::default(),
    )
    .unwrap();
    assert!(brightest(&plain) < 40);
    // No temporary caption scripts or partial outputs are left behind.
    let leftovers: Vec<_> = std::fs::read_dir(&directory)
        .unwrap()
        .filter_map(|entry| entry.unwrap().file_name().into_string().ok())
        .filter(|name| name.starts_with(".tehom-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn speech_audio_is_16k_mono_and_silent_video_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("talk.mp4");
    source(&input, true);
    let wav = directory.path().join("speech.wav");
    tehom_media::extract_speech(&input, &wav, &AtomicBool::new(false)).unwrap();
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=sample_rate,channels",
            "-of",
            "csv=p=0",
        ])
        .arg(&wav)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), "16000,1");
    let silent = directory.path().join("silent.mp4");
    source(&silent, false);
    let error = tehom_media::extract_speech(
        &silent,
        &directory.path().join("x.wav"),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(error.to_string().contains("no sound"), "{error}");
}
