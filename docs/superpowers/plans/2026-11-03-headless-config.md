# Headless configuration

Implement specification section 16 on the existing CLI and shared Automation engine. Preserve CLI-only invocation and saved settings. Add global --config usable after daemon; explicit CLI values override config. Config-relative paths resolve against its containing directory. Read at most 64 KiB; reject unknown keys and invalid intervals before opening the database. Only expose implemented settings, so unsupported concurrency/provider/server options fail explicitly.

1. Add CLI parsing/config tests for precedence, relative paths, invalid and unsupported options; run red.
2. Use existing serde/TOML dependencies to parse data.directory, automation.check_interval_minutes, export.directory, ai.endpoint/model. Connect to existing setters, no new engine.
3. Add documented example and run an actual CLI status command against isolated data; confirm malformed config creates no data. Run workspace formatting, lint and tests before commit.

Service installation, other schema fields and process lifecycle remain separate incomplete requirements.
