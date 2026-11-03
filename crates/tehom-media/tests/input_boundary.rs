#[cfg(unix)]
#[test]
fn fifo_input_is_rejected_without_waiting_for_a_writer() {
    use std::{fs::OpenOptions, sync::mpsc, time::Duration};
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("not-a-video");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let worker_input = input.clone();
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        sender.send(tehom_media::inspect(&worker_input)).unwrap();
    });
    let result = receiver.recv_timeout(Duration::from_millis(500));
    if result.is_err() {
        drop(OpenOptions::new().write(true).open(&input).unwrap());
    }
    worker.join().unwrap();
    assert_eq!(
        result
            .expect("probing waited on a special file")
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
}
