#!/usr/bin/env bash
# Builds whisper-cli from a pinned whisper.cpp release as one self-contained
# program (CPU only, no shared libraries) and puts it, with whisper.cpp's MIT
# licence, into the given directory for packaging/release-archive.sh.
#
#   packaging/build-whisper.sh [output-directory]   (default target/release)
#
# Needs git, CMake and a C/C++ compiler. Any failure stops with an error.
set -euo pipefail
tag=v1.9.5
commit=d1be6fde11ac6e0407606b4e42fe72d34add8037
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
out="${1:-target/release}"
src="target/whisper.cpp-$tag"

if [[ ! -d "$src/.git" ]]; then
    rm -rf "$src"
    git -c advice.detachedHead=false clone --quiet --depth 1 --branch "$tag" https://github.com/ggml-org/whisper.cpp "$src"
fi
[[ "$(git -C "$src" rev-parse HEAD)" == "$commit" ]] || {
    echo "whisper.cpp $tag is not commit $commit" >&2
    exit 1
}

# GGML_NATIVE=OFF: portable x86-64 (AVX2/FMA) or ARMv8 code instead of the
# build machine's CPU. OpenMP off: it would need a separate runtime library.
options=(
    -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DGGML_NATIVE=OFF
    -DGGML_OPENMP=OFF -DGGML_CCACHE=OFF -DWHISPER_BUILD_TESTS=OFF
    -DWHISPER_BUILD_SERVER=OFF -DWHISPER_SDL2=OFF -DWHISPER_CURL=OFF
)
ext=""
case "$(uname -s)" in
    Linux) options+=(-DGGML_STATIC=ON "-DCMAKE_EXE_LINKER_FLAGS=-static") ;;
    # Static C runtime; CMP0091 because whisper.cpp declares CMake 3.5.
    MINGW* | MSYS* | CYGWIN*)
        options+=(-DCMAKE_POLICY_DEFAULT_CMP0091=NEW -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded)
        ext=".exe"
        ;;
esac
# macOS keeps ggml's defaults: Accelerate and Metal are system frameworks and
# the Metal shaders are embedded (GGML_METAL_EMBED_LIBRARY).
cmake -S "$src" -B "$src/build" "${options[@]}"
cmake --build "$src/build" --config Release --target whisper-cli -j "${CARGO_BUILD_JOBS:-2}"

bin="$src/build/bin/whisper-cli$ext"
[[ -f "$bin" ]] || bin="$src/build/bin/Release/whisper-cli$ext" # Visual Studio layout
[[ -f "$bin" ]] || { echo "whisper-cli was not built" >&2; exit 1; }

# Self-contained: nothing outside the operating system may be loaded.
case "$(uname -s)" in
    Linux)
        if readelf -d "$bin" | grep -q NEEDED; then
            readelf -d "$bin" | grep NEEDED >&2
            echo "whisper-cli is not statically linked" >&2
            exit 1
        fi
        ;;
    Darwin)
        if otool -L "$bin" | tail -n +2 | grep -vE '^[[:space:]]*(/usr/lib/|/System/Library/)'; then
            echo "whisper-cli links libraries that macOS does not have" >&2
            exit 1
        fi
        ;;
    *)
        if llvm-readobj --coff-imports "$bin" | grep -iE 'Name: .*(vcruntime|msvcp|vcomp|ucrtbase|api-ms-win-crt|ggml|whisper|gomp|libstdc|libgcc|winpthread)'; then
            echo "whisper-cli needs DLLs that Windows does not have" >&2
            exit 1
        fi
        ;;
esac
"$bin" -h >/dev/null 2>&1 || { echo "whisper-cli does not run" >&2; exit 1; }

mkdir -p "$out"
cp "$bin" "$out/whisper-cli$ext"
cp "$src/LICENSE" "$out/whisper.cpp-LICENSE.txt"
echo "$out/whisper-cli$ext"
