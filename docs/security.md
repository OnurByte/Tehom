# Security boundaries

The prototype accepts local files only. FFmpeg and ffprobe receive argument arrays and canonical input paths; media probing/rendering restrict protocols to local file and pipe. There are no provider credentials or remote AI requests. Render output is staged in a securely created temporary file in the destination directory, synced and published with no-clobber semantics only after successful validation. Cancellation kills and reaps FFmpeg before discarding temporary output.

Future milestones must implement URL/DNS and redirect validation before remote discovery/download, OS-keyring credential storage, explicit remote-provider consent, durable-job recovery and signed update verification. These are not implemented by this prototype. No release security audit or third-party redistribution review has been claimed.
