# Headless user service (Linux)

The desktop and daemon share one engine. Close the desktop before starting the service against the same data directory. The service requires an installed `~/.local/bin/tehom`, FFmpeg/ffprobe and yt-dlp on its PATH. No release installer is available yet; the current binary is built from source.

Copy `config/tehom.example.toml` to `~/.config/tehom/tehom.toml` and select your installed local model. Use absolute data/export paths when installing as a service. Add your channel with the same configuration before starting the daemon:

```sh
tehom --config ~/.config/tehom/tehom.toml add https://www.youtube.com/@your-channel --mode auto-export
mkdir -p ~/.config/systemd/user
cp packaging/linux/tehom.service ~/.config/systemd/user/tehom.service
systemctl --user daemon-reload
systemctl --user enable --now tehom
journalctl --user -u tehom -f
```

These installation commands are documentation; the repository does not enable a service automatically. Stop with `systemctl --user stop tehom`; disable with `systemctl --user disable --now tehom`. A stop signal prevents new scheduler operations, cancels running video jobs and drains the current operation before releasing the data lock. Cancelled jobs require explicit retry. The service manager terminates its entire process group if shutdown exceeds 180 seconds. Live child-process/download/render shutdown coverage remains incomplete.

CLI `status` currently requires exclusive access, so stop the service before inspecting or changing its data. Running continuously after logout requires the host's user-service/linger configuration; this project does not change that setting.
