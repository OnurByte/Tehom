use gstreamer::{self as gst, prelude::*};
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub position: Duration,
    pub duration: Duration,
    pub paused: bool,
    pub volume: f64,
    pub ended: bool,
    pub error: Option<String>,
}

#[derive(Default)]
struct Shared {
    frame: Mutex<Option<Frame>>,
    snapshot: Mutex<Snapshot>,
    stop: AtomicBool,
}

enum Control {
    Paused(bool),
    Volume(f64),
    Seek(Duration),
}

pub struct Preview {
    shared: Arc<Shared>,
    commands: mpsc::SyncSender<Control>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn receive_frame(sample: gst::Sample, shared: &Shared) -> Result<gst::FlowSuccess, gst::FlowError> {
    let caps = sample.caps().ok_or(gst::FlowError::Error)?;
    let info = gst_video::VideoInfo::from_caps(caps).map_err(|_| gst::FlowError::Error)?;
    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
    let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info)
        .map_err(|_| gst::FlowError::Error)?;
    let data = frame.plane_data(0).map_err(|_| gst::FlowError::Error)?;
    let row_bytes = info.width() as usize * 4;
    let stride = usize::try_from(info.stride()[0]).map_err(|_| gst::FlowError::Error)?;
    let mut bgra = Vec::with_capacity(row_bytes * info.height() as usize);
    for row in 0..info.height() as usize {
        bgra.extend_from_slice(
            data.get(row * stride..row * stride + row_bytes)
                .ok_or(gst::FlowError::Error)?,
        );
    }
    *lock(&shared.frame) = Some(Frame {
        width: info.width(),
        height: info.height(),
        bgra,
    });
    Ok(gst::FlowSuccess::Ok)
}

impl Preview {
    /// Call on a background thread: decoder initialization may wait for preroll.
    pub fn open(path: &Path) -> io::Result<Self> {
        gst::init().map_err(io::Error::other)?;
        let uri = url::Url::from_file_path(path.canonicalize()?)
            .map_err(|_| io::Error::other("Geçersiz video yolu"))?;
        let shared = Arc::new(Shared::default());
        let sample_shared = shared.clone();
        let preroll_shared = shared.clone();
        let sink = gst_app::AppSink::builder()
            .caps(
                &gst::Caps::builder("video/x-raw")
                    .field("format", "BGRA")
                    .build(),
            )
            .max_buffers(1)
            .drop(true)
            .enable_last_sample(false)
            .callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |sink| {
                        receive_frame(
                            sink.pull_sample().map_err(|_| gst::FlowError::Eos)?,
                            &sample_shared,
                        )
                    })
                    .new_preroll(move |sink| {
                        receive_frame(
                            sink.pull_preroll().map_err(|_| gst::FlowError::Eos)?,
                            &preroll_shared,
                        )
                    })
                    .build(),
            )
            .build();
        let pipeline = gst::ElementFactory::make("playbin")
            .property("uri", uri.as_str())
            .property("video-sink", &sink)
            .build()
            .map_err(io::Error::other)?;
        let initialization = (|| {
            pipeline
                .set_state(gst::State::Paused)
                .map_err(io::Error::other)?;
            pipeline
                .state(gst::ClockTime::from_seconds(10))
                .0
                .map_err(io::Error::other)?;
            if lock(&shared.frame).is_none() {
                return Err(io::Error::other(
                    "Video görüntüsü çözümlenemedi. Biçimi kontrol et.",
                ));
            }
            Ok(())
        })();
        if let Err(error) = initialization {
            if let Err(shutdown_error) = pipeline.set_state(gst::State::Null) {
                eprintln!("Preview shutdown: {shutdown_error}");
            }
            return Err(error);
        }
        *lock(&shared.snapshot) = Snapshot {
            paused: true,
            volume: 1.0,
            duration: pipeline
                .query_duration::<gst::ClockTime>()
                .map(|t| t.into())
                .unwrap_or_default(),
            ..Snapshot::default()
        };
        let (commands, receiver) = mpsc::sync_channel(16);
        let worker_shared = shared.clone();
        std::thread::Builder::new()
            .name("tehom-preview".into())
            .spawn(move || {
                run_worker(&pipeline, &receiver, &worker_shared);
                if let Err(error) = pipeline.set_state(gst::State::Null) {
                    eprintln!("Preview shutdown: {error}");
                }
            })?;
        Ok(Self { shared, commands })
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.shared.snapshot).clone()
    }
    pub fn take_frame(&self) -> Option<Frame> {
        lock(&self.shared.frame).take()
    }
    pub fn set_paused(&self, paused: bool) -> io::Result<()> {
        self.send(Control::Paused(paused))
    }
    pub fn set_volume(&self, volume: f64) -> io::Result<()> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Ses düzeyi 0 ile 1 arasında olmalı",
            ));
        }
        self.send(Control::Volume(volume))
    }
    pub fn seek(&self, position: Duration) -> io::Result<()> {
        if position > self.snapshot().duration {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Video dışında zaman seçilemez",
            ));
        }
        self.send(Control::Seek(position))
    }
    fn send(&self, control: Control) -> io::Result<()> {
        self.commands.try_send(control).map_err(io::Error::other)
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
    }
}

fn run_worker(pipeline: &gst::Element, receiver: &mpsc::Receiver<Control>, shared: &Shared) {
    let Some(bus) = pipeline.bus() else {
        lock(&shared.snapshot).error = Some("Video olayları okunamadı".into());
        return;
    };
    while !shared.stop.load(Ordering::Acquire) {
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(command) => {
                let result: Result<(), String> = match command {
                    Control::Paused(paused) => {
                        if !paused && lock(&shared.snapshot).ended {
                            if let Err(error) = pipeline.seek_simple(
                                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                                gst::ClockTime::ZERO,
                            ) {
                                lock(&shared.snapshot).error = Some(error.to_string());
                                continue;
                            }
                            lock(&shared.snapshot).ended = false;
                        }
                        pipeline
                            .set_state(if paused {
                                gst::State::Paused
                            } else {
                                gst::State::Playing
                            })
                            .map(|_| ())
                            .map_err(|error| error.to_string())
                    }
                    Control::Volume(volume) => {
                        pipeline.set_property("volume", volume);
                        Ok(())
                    }
                    Control::Seek(position) => {
                        lock(&shared.snapshot).ended = false;
                        pipeline
                            .seek_simple(
                                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                                gst::ClockTime::from_nseconds(position.as_nanos() as u64),
                            )
                            .map_err(|error| error.to_string())
                    }
                };
                if let Err(error) = result {
                    lock(&shared.snapshot).error = Some(error);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        for message in bus.iter() {
            match message.view() {
                gst::MessageView::Error(error) => {
                    lock(&shared.snapshot).error = Some(error.error().to_string());
                }
                gst::MessageView::Eos(_) => {
                    lock(&shared.snapshot).ended = true;
                    if let Err(error) = pipeline.set_state(gst::State::Paused) {
                        lock(&shared.snapshot).error = Some(error.to_string());
                    }
                }
                _ => {}
            }
        }
        let mut snapshot = lock(&shared.snapshot);
        snapshot.position = pipeline
            .query_position::<gst::ClockTime>()
            .map(|t| t.into())
            .unwrap_or(snapshot.position);
        snapshot.duration = pipeline
            .query_duration::<gst::ClockTime>()
            .map(|t| t.into())
            .unwrap_or(snapshot.duration);
        snapshot.volume = pipeline.property::<f64>("volume");
        snapshot.paused = pipeline.state(gst::ClockTime::ZERO).1 == gst::State::Paused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::Command,
        sync::Arc,
        time::{Duration, Instant},
    };

    #[test]
    fn decode_pause_seek_and_shutdown() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("odd-stride.mp4");
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
                    "testsrc2=size=162x94:rate=15",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440",
                    "-t",
                    "3",
                    "-c:v",
                    "libx264",
                    "-c:a",
                    "aac"
                ])
                .arg(&source)
                .status()
                .unwrap()
                .success()
        );
        let preview = Preview::open(&source).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let frame = loop {
            if let Some(frame) = preview.take_frame() {
                break frame;
            }
            assert!(
                Instant::now() < deadline,
                "no decoded frame: {:?}",
                preview.snapshot()
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!((frame.width, frame.height), (162, 94));
        assert_eq!(frame.bgra.len(), 162 * 94 * 4);
        assert!(preview.snapshot().paused);
        assert!(preview.set_volume(f64::NAN).is_err());
        assert!(preview.set_volume(-0.1).is_err());
        assert!(preview.set_volume(1.1).is_err());
        preview.set_volume(0.25).unwrap();
        let volume_deadline = Instant::now() + Duration::from_secs(2);
        while (preview.snapshot().volume - 0.25).abs() > 0.001 {
            assert!(
                Instant::now() < volume_deadline,
                "volume did not reach the audio pipeline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        preview.seek(Duration::from_millis(1500)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while preview
            .snapshot()
            .position
            .abs_diff(Duration::from_millis(1500))
            > Duration::from_millis(150)
        {
            assert!(
                Instant::now() < deadline,
                "seek failed: {:?}",
                preview.snapshot()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        preview.set_paused(false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while preview.snapshot().position < Duration::from_millis(1800) {
            assert!(
                Instant::now() < deadline,
                "play failed: {:?}",
                preview.snapshot()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        preview.set_paused(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !preview.snapshot().paused {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let weak = Arc::downgrade(&preview.shared);
        drop(preview);
        let deadline = Instant::now() + Duration::from_secs(3);
        while weak.upgrade().is_some() {
            assert!(
                Instant::now() < deadline,
                "preview worker leaked after close"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
