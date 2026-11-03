#[test]
fn checks_real_volume_and_rejects_unavailable_space() {
    let directory = tempfile::tempdir().unwrap();
    assert!(tehom_media::require_space(directory.path(), 0).is_ok());
    let error = tehom_media::require_space(directory.path(), u64::MAX).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::StorageFull);
    assert!(tehom_media::require_space(&directory.path().join("missing"), 1).is_err());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}
