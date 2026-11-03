# Local model validation

2026-11-03 Linux x86_64. Test runtimes and model weights are confined to the task scratch directory; no system service or user model configuration is installed. The inference endpoint binds only 127.0.0.1:18081.

Runtime: official llama.cpp b12064, version 0.8.0-dev, commit 51fe42c5d, CPU inference. Initial model: official Qwen/Qwen2.5-0.5B-Instruct-GGUF Q4_K_M, SHA-256 74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db.

Input: a generated 60-second color-video fixture and an explicitly authored three-part transcript about testing backups. This is transport/analysis/export validation with synthetic input, not evidence of audiovisual understanding or a real speech transcript.

Without structured-output constraints, the small model echoed the transcript instead of producing clip JSON. Tehom rejected the response and produced no drafts or exports. With JSON-schema constraints, the same model returned no usable candidates; the job stopped for input and no arbitrary clip was created. The application now requests schema-constrained output through Ollama format or OpenAI-compatible response_format, while retaining its independent validation of timestamps, duration and overlap. Both request shapes are checked by real loopback HTTP integration tests.

References: [llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md), [Ollama structured outputs](https://docs.ollama.com/capabilities/structured-outputs), [official Qwen model](https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF).


## Successful model-to-MP4 run

The same input was retried with official Qwen/Qwen2.5-1.5B-Instruct-GGUF Q4_K_M (SHA-256 6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e), using the same loopback llama.cpp server and the new schema request. The actual model selected three non-overlapping intervals: 0–20s, 20–40s and 40–60s. The engine independently validated them, persisted all three drafts and exported all three MP4s. The job finished `complete` with no error.

All three outputs were separately checked with ffprobe: H.264, 1080×1920, 20 seconds each within 0.25-second tolerance. Titles corresponded to testing restores, comparing restored files and keeping a disconnected backup. This proves an actual local model → validated candidates → persistent drafts → FFmpeg MP4 flow. It does not establish clipping quality on natural long videos, multilingual performance or transcript faithfulness to real speech. The test server was stopped after validation; no service remains installed or running.
