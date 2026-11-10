# JSON-processing repair — work in progress (2026-11-10)

Branch: fix/provider-json-processing. Baseline: 526bc4f. Release candidate: 0.5.1 (not published).

## Reproduced root cause: quota-dependent process capture

The original source's EOF was reproduced five out of five times with both the
installed 0.5.0 binary and the 0.5.1 candidate in the default environment. The
new stage diagnostics identified FFprobe media metadata, before any transcript
or Gemini request. A direct pipe capture returned 285 bytes of valid JSON.
An explicit /tmp write failed with errno 122 (EDQUOT); actual FFprobe with stdout
redirected to an anonymous /tmp file exited 0 with a zero-byte capture and no
stderr. Python selected /var/tmp as its writable temporary-directory fallback,
which initially obscured the comparison. Overriding the candidate's TMPDIR to
on-disk storage also made inspection pass; this is diagnostic evidence, not the
final workaround used for the release gate.

Baseline crates/tehom-media/src/lib.rs:830–831 created stdout/stderr capture files
in the default temporary directory. FFprobe's successful exit did not prove its
buffered stdout write succeeded; the empty capture reached the JSON decoder.
The repair reads both streams concurrently through bounded pipes, retains the
64 KiB-per-stream limit and kill/reap behavior, and moves render progress logs
to the validated export destination. It does not change the host's /tmp quota
or erase any existing temporary/user files.

A new isolated Linux integration test creates a real MP4 first, then uses
RLIMIT_FSIZE=0 with SIGXFSZ ignored to reproduce failed capture writes without
killing FFprobe. It failed at FFprobe media metadata Eof before the pipe repair
and passed afterward; actual metadata and export-identity reads succeed and
the input is byte-identical. The first standalone attempt used the full /tmp
and failed during fixture generation; the intended red result was obtained with
the fixture on disk and test stdout/stderr captured through pipes. Nine media
unit tests also passed, including simultaneous stream overflow, cancellation,
timeout, atomic export and stage diagnostics. Strict media-only Clippy first
found a needless borrow in the new cancellation test; it was corrected and
cargo clippy -p tehom-media --all-targets -- -D warnings passed. After rebuilding, /tmp still returned EDQUOT while the fixed candidate
successfully inspected the original source five out of five times, with no
TMPDIR override. The complete original-source workflow was then restarted with
that candidate; its final result is pending.

## Reproduction

Before modifying production code, added
gemini_skips_thought_parts_and_reports_empty_selection_stage.
Ran cargo test -p tehom-automation gemini_skips_thought -- --nocapture.
It compiled and failed at the remote prompt/schema consistency assertion
(tests.rs:3433 before formatting): "remote prompt must match schema".
The following cases in that loop were not reached in this baseline run.

## Findings and repair locations

The process-capture quota failure is now reproduced on the original source. The following additional source defects and repair boundaries were also verified.

| Defect/boundary | Current source location | Repair |
| --- | --- | --- |
| Root-array prompt vs remote object schema | crates/tehom-automation/src/lib.rs:539 and :3280 | Remote instructions request an object containing clips; local arrays remain supported. |
| Gemini first-part parsing and incomplete output | crates/tehom-automation/src/lib.rs:3394 | Skip thought parts, join output text, diagnose missing candidates/parts, empty output and non-STOP finishes. |
| Empty/truncated/malformed provider JSON and invalid clip fields | crates/tehom-automation/src/lib.rs:732 | Stage, category, line and column without echoing rejected private values. |
| Recoverable status/transport failures | crates/tehom-automation/src/lib.rs:748 | Three attempts at most; authentication, unavailable models, completed invalid JSON and size violations do not retry. Long RetryInfo/Retry-After delays stop early. |
| Error chain lost in job history | crates/tehom-automation/src/lib.rs:2035 | Persist safe full chain and classify nested Needs input errors. |
| Completed chunks lost when a later chunk fails | crates/tehom-automation/src/lib.rs:2226 and :3249 | SQLite checkpoint per video/index, keyed by exact AI config, captured settings and input. Complete drafts remain transactional before render. |
| Quota-dependent process capture | crates/tehom-media/src/lib.rs:835 and :923 (baseline :830–831) | Bounded concurrent stdout/stderr pipes; no capture-file writes. Render progress files use the export parent. |
| FFprobe metadata/export-marker JSON | crates/tehom-media/src/lib.rs:99, :596 and :812 | Name the FFprobe stage and report safe parse position/category. |
| Generated speech SRT encoding | crates/tehom-automation/src/lib.rs:2870 (baseline :2752) | Normalize invalid generated characters, retain SRT validation, commit repaired SRT and a safe caption-review warning together. User uploads remain strict. |
| Saved AI settings decoded in worker snapshot | crates/tehom-automation/src/lib.rs:4417 | Name the stored-settings stage without disclosing its rejected values. |

## EOF boundary audit

Direct JSON boundaries on the processing path include remote response envelopes,
Gemini/OpenAI selection text, local response envelope and selection text,
Claude CLI envelope/result, Codex answer file, and FFprobe inspect/export-marker
JSON. Local model listing also parses JSON, but is not the video worker path.
Saved AI settings are also decoded while building the worker snapshot and now
use the same safe stage diagnostics. Other settings and saved clip JSON columns
use fallbacks and do not propagate EOF. Kick discovery and YouTube channel
metadata parsing happen during source discovery, not run_video; local/remote
model-list JSON belongs to the model picker. Stored Instagram session JSON is
parsed only for the separately reported optional auto-publication after export;
that integration is unchanged. Neither discovery nor publishing explains an EOF
before any draft/transcript exists in the original saved job.
Do not log full bodies, transcripts, credentials, or reasoning to diagnose these.

## Additional live-provider regressions (2026-11-11)

The live provider exposed two more cases. Added
ai_rate_limit_long_delay_is_not_retried_early (a 3600s RetryInfo must cause one
request and a safe delay diagnostic) and
ai_missing_model_requires_input_without_echoing_provider_body. Both failed
before their fixes. The patch now respects long provider RetryInfo delays
without sleeping for hours and identifies HTTP 404 as a model/endpoint
prerequisite. These tests passed in the 9-test targeted AI run.

## Generated speech encoding boundary

The real-source run exposed an additional failure at transcribe(): strict UTF-8
conversion rejected the generated SRT before any transcript could be saved.
The pinned upstream [Whisper CLI SRT writer](https://github.com/ggml-org/whisper.cpp/blob/d1be6fde11ac6e0407606b4e42fe72d34add8037/examples/cli/cli.cpp#L502)
writes segment text directly. The repair decodes generated speech with explicit
replacement characters, keeps subtitle/timestamp validation, and atomically
persists the repaired SRT and a fixed warning asking the user to review captions.
User-uploaded files remain strictly decoded and unchanged. The generated SRT,
not the normalized transcript representation, is saved for subsequent reads.
No private speech text is included in the warning or this report.

## Current verification

- cargo fmt --check passed after the initial patch.
- Gemini prompt/parts regression passed after the initial patch.
- Recoverable-status, finite-budget and chunk-reopen regressions first failed,
  then passed. The actual-media chunk test also injected an export collision,
  preserved both committed drafts and the unrelated file, resumed and verified
  two 25-second 160×90 MP4s using FFprobe.
- Empty FFprobe output was injected in a subprocess using an actual generated
  MP4: the test failed before the stage diagnostics and passed afterward.
- Independent review found a captured-settings/digest mismatch and missing
  mid-body transport retries. Both were demonstrated as failing regressions;
  both fixes passed their regressions in the 75-test automation run. A subsequent
  review of the later live-quota diagnostic branch found that a disconnected
  429/503 error body can bypass retry handling. The expanded disconnected-body regression covers 200, 429, 503 and a long
  Retry-After; it failed on a disconnected 429 body before the fix. After sharing
  body-read handling, cargo test -p tehom-automation --lib ai_ -- --nocapture
  passed all 9 selected tests. Follow-up review found no remaining helper issue.
- One full automation run was stopped after parallel fixture failures and a
  remaining test blocked in accept/futex (not counted as passing). A subsequent
  two-thread run uses on-disk TMPDIR; one encoder was SIGKILLed by the host.
  It ended with 74 passed and 1 failed (the SIGKILL export-progress case).
  Kernel and systemd-oomd journal queries returned no matching evidence, so
  the origin of SIGKILL is not established. Final workspace checks remain pending. The later Clippy process was
  deliberately interrupted (exit 130) to let the pending regression compile
  before the final fix; the queued workspace test was temporarily suspended
  at its build lock. That pre-version-bump process was subsequently discarded
  (exit 137) and cargo test --workspace --locked was started afresh against the
  final 0.5.1 manifest graph. None of the interrupted runs is a passing check.
- A temporary /tmp copy of the source hit Disk quota exceeded, also causing a
  test run to fail with SQLite I/O errors. The scratch data was moved to disk;
  the same targeted tests then passed without code changes.
- The original saved job contained only the bare EOF message; later repetition on the same source identified the FFprobe capture/quota boundary described above. The retained source is a real 875.740590-second 3840×2160
  AV1/AAC file. Both FFprobe and the installed application's import path currently
  inspect it successfully. An isolated copy reaches needs_input because no
  captions or speech model are available. The user has now explicitly authorized
  downloading the base speech model for this source. The isolated model download
  completed and validated its GGML header and 147951465-byte size; SHA-256 is
  60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe.
  The copied original source ran actual speech extraction and whisper.cpp, but
  failed before transcript persistence with invalid UTF-8 at byte offset 19530.
  There were no committed transcripts/drafts and no Gemini selection request
  in that run; the command exit status was 0 while the database job state was
  failed. This is not a successful integration. A corresponding injected-byte
  regression using actual FFmpeg media failed before the speech-boundary fix;
  cargo test -p tehom-automation --lib speech_invalid_utf8 -- --nocapture then
  passed its repaired-output, warning and strict-upload assertions. The first rebuilt candidate then reproduced the original FFprobe EOF under
  the default temporary-directory quota. A real original-source rerun with the
  new pipe capture and repaired speech boundary remains required.
- A separate actual public video (TED, YouTube iG9CE55wbtY, 1203.414 seconds,
  480×360) was downloaded with its English subtitles and imported into isolated
  application data. Gemini 3.5 Flash actually analyzed its two transcript chunks,
  producing five committed drafts. The first portrait MP4 was independently
  inspected with FFprobe: H.264 + AAC, 1080×1920, 31.900 seconds against a
  31.872-second intended range. A decoded frame at 10 seconds was visually
  inspected and shows the English captions burned into the picture. To concentrate resources on the original video after the speech-model
  authorization, the optional public-video job was deliberately terminated and
  marked cancelled after its first validated output. Its source, five committed
  drafts and one published MP4 were rechecked and retained. This is partial
  actual generated-media evidence, not a completed public-video workflow or a
  provider fixture/compilation claim.
- The configured gemini-2.5-pro appears in provider metadata but generateContent
  currently returns HTTP 404/NOT_FOUND with an unavailable-model diagnostic.
  gemini-pro-latest returns 429 with a provider RetryInfo delay of 37855 seconds
  and free-tier quota violations. Neither is a successful integration. Alternate
  models were selected only in scratch data; the user's original settings remain
  unchanged. No credentials, transcripts or full model responses were printed.
- No release/push/merge performed: the requested release gate is not satisfied.

## Remaining gates

1. Verify the speech encoding regression and rerun the actual source workflow; verify malformed
   envelopes, truncated selection JSON, missing/empty candidates, reasoning
   parts, non-success finish reasons, and rate limiting.
2. Add bounded recoverable retries without retrying permanent/auth failures.
3. Preserve successful per-chunk analysis across retry; retain media and drafts.
4. Add actual FFmpeg failure-injection integration coverage.
5. Finish stage diagnostics including FFprobe and persist safe full error chains.
6. Run fmt, workspace clippy and tests and record real outcomes.
7. Inspect configured provider without exposing credentials; run a real long-form
   workflow only with the required consent/authorization.
8. Review, push, merge and publish only after the release gate succeeds.
