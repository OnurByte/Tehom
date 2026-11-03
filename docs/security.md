# Security boundaries

Channel inputs accept only validated YouTube channel IDs or HTTPS YouTube channel/handle URLs. Credentials, explicit ports, unrelated/private hosts and path-normalization tricks are rejected. RSS endpoints are fixed by the application; response sizes, redirects and timeouts are controlled. Local AI endpoints are restricted to loopback, disable proxy inheritance and redirects, and accept no embedded credentials. No cloud provider or API key is currently used.

FFmpeg, ffprobe and yt-dlp receive argument arrays. Local media inspection rejects special files, restricts protocols, bounds subprocess output and supports timeouts/cancellation. Completed MP4 publication uses secure temporary files and no-clobber semantics. Automatic exports include a persisted random ownership identity; interrupted publication is adopted only after identity, geometry and duration verification, preserving unrelated existing files.

SQLite transactions preserve candidate batches before any render. A lifetime file lock prevents duplicate engine ownership. Cancellation cannot be overwritten by a final completed-state update. Missing transcripts never trigger ASR or arbitrary exports.

Remaining release work includes cloud-provider consent/keyring support, stronger downloader process-tree lifecycle checks on all systems, migration/version policy, signing/update integrity, independent security review and third-party redistribution/license audit. Current CI evidence is not a release security certification.
