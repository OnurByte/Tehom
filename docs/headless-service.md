# Headless user service (Linux)

The desktop and daemon share one engine. Close the desktop before starting the service against the same data directory. The service requires an installed `~/.local/bin/tehom`, FFmpeg/ffprobe and yt-dlp on its PATH. No release installer is available yet; the current binary is built from source.

Copy `config/tehom.example.toml` to `~/.config/tehom/tehom.toml` and select your installed local model. Use absolute data/export paths when installing as a service. Add your channel with the same configuration before starting the daemon:

```sh
tehom --config ~/.config/tehom/tehom.toml add https://www.youtube.com/@your-channel --mode auto-export
tehom service install --config ~/.config/tehom/tehom.toml --enable
journalctl --user -u tehom -f
```

`tehom service install` validates the configuration, then writes `$XDG_CONFIG_HOME/systemd/user/tehom.service` (normally `~/.config/systemd/user`) pointing at the running `tehom` binary and the absolute configuration path. It never replaces an existing unit that differs; rerunning with the same binary and configuration is a no-op. Without `--enable` it only prints the `systemctl --user` commands to run. `tehom service uninstall --disable` stops and disables the service, then removes the unit only if Tehom generated it. Paths containing quotes, backslashes, `%` or `$` are rejected rather than escaped. `packaging/linux/tehom.service` remains as a manual template.

The generated unit passed `systemd-analyze --user verify` with systemd 262 on Arch Linux. Automated tests cover installation, refusal to overwrite, and uninstall against a temporary `XDG_CONFIG_HOME`. They do not call `systemctl`; the `--enable`/`--disable` paths have not been exercised against a live user manager.

The service is enabled only when you pass `--enable`. Stop with `systemctl --user stop tehom`; disable with `systemctl --user disable --now tehom`. A stop signal prevents new scheduler operations, cancels running video jobs and drains the current operation before releasing the data lock. Cancelled jobs require explicit retry. The service manager terminates its entire process group if shutdown exceeds 180 seconds. The CLI integration test starts an actual fixture downloader process, stops the daemon with SIGTERM, verifies the downloader exited and checks that the cancelled job remains cancelled after reopening. Descendant-process trees and active render shutdown remain unverified.

CLI `status` reads while the service is running. Other management commands still need exclusive access; stop the service before changing its data through those commands. Running continuously after logout requires the host's user-service/linger configuration; this project does not change that setting.
