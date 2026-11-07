//! Output profiles and rotation-aware geometry, with real FFmpeg renders.
use std::{path::Path, process::Command, sync::atomic::AtomicBool};
use tehom_media::Framing;

fn source(path: &Path, size: &str, extra: &[&str]) {
    assert!(
        Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("testsrc2=size={size}:rate=10"))
            .args(["-t", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
            .args(extra)
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn profiles_render_expected_geometry_and_rotation_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let landscape = dir.path().join("landscape.mp4");
    source(&landscape, "320x180", &[]);
    let cases = [
        (
            "vertical",
            Framing::Fill {
                width: 1080,
                height: 1920,
            },
            (1080, 1920),
        ),
        (
            "horizontal",
            Framing::Fill {
                width: 1920,
                height: 1080,
            },
            (1920, 1080),
        ),
        ("original", Framing::Source, (320, 180)),
    ];
    for (name, framing, expected) in cases {
        let output = dir.path().join(format!("{name}.mp4"));
        tehom_media::export_framed(
            &landscape,
            &output,
            0.0,
            1.0,
            &AtomicBool::new(false),
            "id-1",
            framing,
        )
        .unwrap();
        let info = tehom_media::inspect(&output).unwrap();
        assert_eq!((info.width, info.height), expected, "{name}");
        assert_eq!(
            tehom_media::export_identity(&output).unwrap().as_deref(),
            Some("id-1")
        );
    }

    // Odd source sizes are trimmed to even dimensions for H.264.
    let odd = dir.path().join("odd.mp4");
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
                "testsrc2=size=161x91:rate=10",
                "-t",
                "2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv444p"
            ])
            .arg(&odd)
            .status()
            .unwrap()
            .success()
    );
    let trimmed = dir.path().join("odd-original.mp4");
    tehom_media::export_framed(
        &odd,
        &trimmed,
        0.0,
        1.0,
        &AtomicBool::new(false),
        "id-2",
        Framing::Source,
    )
    .unwrap();
    let info = tehom_media::inspect(&trimmed).unwrap();
    assert_eq!((info.width, info.height), (160, 90));

    // A phone-style rotated file reports and renders its display geometry.
    let rotated = dir.path().join("rotated.mp4");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-display_rotation",
                "90",
                "-i"
            ])
            .arg(&landscape)
            .args(["-c", "copy"])
            .arg(&rotated)
            .status()
            .unwrap()
            .success()
    );
    let info = tehom_media::inspect(&rotated).unwrap();
    assert_eq!((info.width, info.height), (180, 320));
    let upright = dir.path().join("rotated-original.mp4");
    tehom_media::export_framed(
        &rotated,
        &upright,
        0.0,
        1.0,
        &AtomicBool::new(false),
        "id-3",
        Framing::Source,
    )
    .unwrap();
    let info = tehom_media::inspect(&upright).unwrap();
    assert_eq!((info.width, info.height), (180, 320));
}

#[test]
fn render_progress_rises_monotonically_to_completion() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("long.mp4");
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
                "12",
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
    let reports = std::sync::Mutex::new(Vec::new());
    let record = |fraction: f64| reports.lock().unwrap().push(fraction);
    tehom_media::export_framed_observed(
        &input,
        &dir.path().join("clip.mp4"),
        0.0,
        12.0,
        &AtomicBool::new(false),
        "progress-1",
        Framing::Fill {
            width: 1080,
            height: 1920,
        },
        Some(&record),
    )
    .unwrap();
    let reports = reports.into_inner().unwrap();
    assert!(reports.len() >= 2, "{reports:?}");
    assert!(
        reports.windows(2).all(|pair| pair[0] <= pair[1]),
        "{reports:?}"
    );
    assert!(
        reports.iter().any(|f| *f > 0.0 && *f < 1.0),
        "no intermediate progress: {reports:?}"
    );
    assert_eq!(reports.last(), Some(&1.0));
    // No progress log is left in the system temporary folder for this render.
    assert!(!dir.path().read_dir().unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tehom-")
    }));
}
