#!/usr/bin/env bash
# Install the iris and iris-ci command-line binaries, either from a published
# irix7/iris release or by building a checkout.
#
# This is the one implementation of the install logic. The composite action in
# .github/actions/setup-iris calls it; it is also meant to be run by hand, for
# example on a CI host that is not running GitHub Actions.
#
#   tools/install-iris-ci.sh [options]
#
#   --version V         latest (default), or a release version with or without
#                       the leading v: v0.1.6+irix7, 0.1.6+irix7.
#   --from-source       build iris and iris-ci instead of downloading a release
#   --path DIR          checkout to build (default: .)
#   --features LIST     cargo features for --from-source
#                       (default: lightning,gpujit)
#   --dest DIR          install directory (default: $RUNNER_TEMP/iris when
#                       GitHub Actions set RUNNER_TEMP, otherwise
#                       $XDG_DATA_HOME/iris or ~/.local/share/iris)
#   --install-deps      Linux: apt-get the shared libraries the release needs,
#                       plus the build packages when --from-source is given
#   --smoke             run `iris --help` and `iris-ci --help` from a clean
#                       environment after installing
#   --repo OWNER/NAME   source repository (default: irix7/iris)
#   -h, --help          print this help
#
# Results are printed as KEY=VALUE lines. In GitHub Actions the same values are
# also appended to $GITHUB_OUTPUT, the install directory is appended to
# $GITHUB_PATH, and a summary goes to $GITHUB_STEP_SUMMARY.
#
# Release tags use SemVer build metadata (`v0.1.6+irix7`) but GitHub rewrites
# `+` to `.` in asset names (`IRIS-cli-linux-x64-0.1.6.irix7.tar.gz`), so the
# tag and the asset version are derived separately below.
set -euo pipefail

REPO=irix7/iris
VERSION_INPUT=latest
FROM_SOURCE=0
SOURCE=.
FEATURES=lightning,gpujit
DEST=""
INSTALL_DEPS=0
SMOKE=0

# X.Y.Z with optional SemVer build metadata. The fork's release tags add
# `+irix7`; allowing any build metadata keeps this useful for a renamed fork.
RELEASE_RE='^[0-9]+\.[0-9]+\.[0-9]+(\+[0-9A-Za-z.-]+)?$'

log()  { printf '==> %s\n' "$*" >&2; }
warn() {
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
        printf '::warning::%s\n' "$*" >&2
    else
        printf 'warning: %s\n' "$*" >&2
    fi
}
die() {
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
        printf '::error::%s\n' "$*" >&2
    else
        printf 'error: %s\n' "$*" >&2
    fi
    exit 1
}
need() {
    local cmd
    for cmd in "$@"; do
        command -v "$cmd" >/dev/null 2>&1 || die "'$cmd' is required but not installed"
    done
}

usage() {
    sed -n '2,33p' "$0" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

native_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$1"
    else
        printf '%s\n' "$1"
    fi
}

# "latest" is resolved through the API so the tag and the asset name come from
# the same answer. A rate-limited or blocked API falls back to the redirect on
# /releases/latest, which needs no token and no JSON.
resolve_latest() {
    local api json tag effective token
    api="https://api.github.com/repos/$REPO/releases/latest"
    token="${IRIS_GITHUB_TOKEN:-${GITHUB_TOKEN:-${GH_TOKEN:-}}}"

    if [ -n "$token" ]; then
        json=$(curl -fsSL --retry 3 --retry-delay 2 \
            -H "Accept: application/vnd.github+json" \
            -H "Authorization: Bearer $token" "$api" 2>/dev/null) || json=
    else
        json=$(curl -fsSL --retry 3 --retry-delay 2 \
            -H "Accept: application/vnd.github+json" "$api" 2>/dev/null) || json=
    fi

    tag=""
    if [ -n "$json" ]; then
        tag=$(printf '%s\n' "$json" |
            sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
            head -n 1 || true)
    fi
    if [ -z "$tag" ]; then
        warn "GitHub API did not return a release tag; using the /releases/latest redirect"
        effective=$(curl -fsSIL -o /dev/null -w '%{url_effective}' \
            "https://github.com/$REPO/releases/latest" 2>/dev/null) || effective=
        tag=${effective##*/tag/}
        tag=${tag//%2B/+}
        tag=${tag//%2b/+}
        tag=${tag%%\?*}
        [ -n "$tag" ] && [ "$tag" != "$effective" ] \
            || die "could not resolve the latest $REPO release; pass --version vX.Y.Z[+build]"
    fi
    tag=${tag#v}
    [[ "$tag" =~ $RELEASE_RE ]] \
        || die "latest resolves to '$tag', which is not a X.Y.Z[+build] release"
    printf '%s\n' "$tag"
}

normalise_version() {
    local v
    v="${1#refs/tags/}"
    v="${v#v}"
    [[ "$v" =~ $RELEASE_RE ]] \
        || die "'$1' is not a release version; want X.Y.Z or X.Y.Z+build, or latest"
    printf '%s\n' "$v"
}

apt_install() {
    local pkg
    for pkg in "$@"; do
        if ! apt-cache show "$pkg" >/dev/null 2>&1; then
            warn "install-deps: package '$pkg' is not available on this distribution"
            continue
        fi
        log "installing $pkg"
        $SUDO env DEBIAN_FRONTEND=noninteractive \
            apt-get install -y --no-install-recommends "$pkg" >/dev/null
    done
}

install_deps() {
    if [ "$OS" != linux ]; then
        log "install-deps: nothing to install on $OS"
        return 0
    fi
    if ! command -v apt-get >/dev/null 2>&1; then
        warn "install-deps: apt-get not found; install libGL and ALSA (libasound)"
        warn "install-deps: with your package manager, or set install-deps: false"
        return 0
    fi
    SUDO=""
    if [ "$(id -u)" -ne 0 ]; then
        command -v sudo >/dev/null 2>&1 \
            || die "install-deps: running as a non-root user and sudo is not available"
        SUDO=sudo
    fi
    log "apt-get update"
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get update -qq
    # The release CLI links libGL and ALSA directly; EGL, xkbcommon and the
    # video/GL loaders are dlopen'd at run time. libstdc++ is a direct
    # dependency of the release link too.
    apt_install libgl1 libegl1 libxkbcommon0 libstdc++6
    # 24.04 renamed libasound2 to libasound2t64 (time_t transition). Older
    # releases, and Debian, keep libasound2.
    if apt-cache show libasound2t64 >/dev/null 2>&1; then
        apt_install libasound2t64
    else
        apt_install libasound2
    fi
    if [ "$FROM_SOURCE" = 1 ]; then
        apt_install pkg-config libasound2-dev libgl-dev
    fi
}

install_binaries() {  # <source dir>
    local src="$1"
    [ -f "$src/iris$EXE" ]    || die "no iris$EXE in $src"
    [ -f "$src/iris-ci$EXE" ] || die "no iris-ci$EXE in $src"
    mkdir -p "$DEST"
    cp -f "$src/iris$EXE"    "$DEST/iris$EXE"
    cp -f "$src/iris-ci$EXE" "$DEST/iris-ci$EXE"
    chmod 0755 "$DEST/iris$EXE" "$DEST/iris-ci$EXE" 2>/dev/null || true
    IRIS_PATH="$DEST/iris$EXE"
    CI_PATH="$DEST/iris-ci$EXE"
}

check_libs() {
    [ "$OS" = linux ] || return 0
    command -v ldd >/dev/null 2>&1 || return 0
    local missing
    missing=$(ldd "$IRIS_PATH" 2>/dev/null |
        awk '/not found/ {print $1}' | sort -u | tr '\n' ' ' || true)
    [ -z "$missing" ] && return 0
    warn "iris is missing shared libraries: ${missing% }"
    warn "Ubuntu: install libgl1 and libasound2t64 (or libasound2), or pass --install-deps"
}

clean_run() {  # <binary> <args...>
    local bin="$1"
    shift
    if [ "$OS" = windows ]; then
        # Git Bash's env -i would also clear the Windows variables the loader
        # and MSYS runtime expect. The point of the smoke test is that the
        # binary itself starts, which a plain run proves on Windows.
        "$bin" "$@" >/dev/null
    else
        env -i "$bin" "$@" >/dev/null
    fi
}

smoke_test() {
    log "smoke: $IRIS_PATH --help"
    clean_run "$IRIS_PATH" --help \
        || die "smoke test failed: $IRIS_PATH --help (missing shared library?)"
    log "smoke: $CI_PATH --help"
    clean_run "$CI_PATH" --help \
        || die "smoke test failed: $CI_PATH --help (missing shared library?)"
}

emit() {  # <key> <value>
    printf '%s=%s\n' "$1" "$2"
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
        printf '%s=%s\n' "$1" "$2" >> "$GITHUB_OUTPUT"
    fi
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)      VERSION_INPUT="$2"; shift 2 ;;
        --from-source)  FROM_SOURCE=1; shift ;;
        --path)         SOURCE="$2"; shift 2 ;;
        --features)     FEATURES="$2"; shift 2 ;;
        --dest)         DEST="$2"; shift 2 ;;
        --install-deps) INSTALL_DEPS=1; shift ;;
        --smoke)        SMOKE=1; shift ;;
        --repo)         REPO="$2"; shift 2 ;;
        -h|--help)      usage ;;
        *)              die "unknown option: $1 (try --help)" ;;
    esac
done

case "$(uname -s)" in
    Linux)              OS=linux ;;
    Darwin)             OS=macos ;;
    MINGW*|MSYS*|CYGWIN*) OS=windows ;;
    *) die "unsupported platform '$(uname -s)'; setup-iris supports Linux, macOS and Windows" ;;
esac
case "$(uname -m)" in
    x86_64|amd64) ARCH=x64 ;;
    arm64|aarch64) ARCH=arm64 ;;
    *) die "unsupported architecture '$(uname -m)' on $OS; setup-iris supports x86_64 and arm64" ;;
esac
EXE=""
[ "$OS" = windows ] && EXE=.exe

# GitHub publishes one universal CLI tarball for macOS; Linux and Windows
# assets are per-architecture.
case "$OS" in
    linux)   ASSET_OS=linux;   ASSET_ARCH=$ARCH ;;
    macos)   ASSET_OS=macos;   ASSET_ARCH=universal ;;
    windows) ASSET_OS=windows; ASSET_ARCH=$ARCH ;;
esac

if [ -z "$DEST" ]; then
    if [ -n "${RUNNER_TEMP:-}" ]; then
        DEST="$RUNNER_TEMP/iris"
    elif [ -n "${XDG_DATA_HOME:-}" ]; then
        DEST="$XDG_DATA_HOME/iris"
    elif [ -n "${HOME:-}" ]; then
        DEST="$HOME/.local/share/iris"
    else
        die "no --dest, RUNNER_TEMP, XDG_DATA_HOME or HOME; pass --dest DIR"
    fi
fi
mkdir -p "$DEST"
DEST=$(cd "$DEST" && pwd)

WORK=$(mktemp -d "${TMPDIR:-/tmp}/iris-install.XXXXXX")
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

if [ "$INSTALL_DEPS" = 1 ]; then
    install_deps
fi

if [ "$FROM_SOURCE" = 1 ]; then
    [ -d "$SOURCE" ] || die "--from-source: checkout '$SOURCE' does not exist"
    SOURCE=$(cd "$SOURCE" && pwd)
    [ -f "$SOURCE/Cargo.toml" ] \
        || die "--from-source: no Cargo.toml in '$SOURCE'"
    grep -Eq '^[[:space:]]*name[[:space:]]*=[[:space:]]*"iris"' "$SOURCE/Cargo.toml" \
        || die "--from-source: '$SOURCE' is not an IRIS checkout (no iris package)"
    need cargo
    log "building iris and iris-ci from $SOURCE (features: ${FEATURES:-<default>})"
    if [ -n "$FEATURES" ]; then
        ( cd "$SOURCE" && cargo build --release -p iris --bin iris --bin iris-ci \
            --features "$FEATURES" )
    else
        ( cd "$SOURCE" && cargo build --release -p iris --bin iris --bin iris-ci )
    fi
    if [ -n "${CARGO_TARGET_DIR:-}" ]; then
        case "$CARGO_TARGET_DIR" in
            /*|[A-Za-z]:[\\/]*) TARGET_BASE="$CARGO_TARGET_DIR" ;;
            *)  TARGET_BASE="$SOURCE/$CARGO_TARGET_DIR" ;;
        esac
    else
        TARGET_BASE="$SOURCE/target"
    fi
    RELEASE_DIR=""
    for candidate in "$TARGET_BASE/release" "$TARGET_BASE"/*/release; do
        if [ -x "$candidate/iris$EXE" ] && [ -x "$candidate/iris-ci$EXE" ]; then
            RELEASE_DIR="$candidate"
            break
        fi
    done
    [ -n "$RELEASE_DIR" ] \
        || die "cargo finished but no iris$EXE and iris-ci$EXE were found under $TARGET_BASE"
    install_binaries "$RELEASE_DIR"
    if command -v git >/dev/null 2>&1 \
        && git -C "$SOURCE" rev-parse --short=8 HEAD >/dev/null 2>&1; then
        VERSION="dev.$(git -C "$SOURCE" rev-parse --short=8 HEAD)"
    else
        VERSION=dev
    fi
else
    need curl
    case "$OS" in windows) ;; *) need tar ;; esac
    input_lc=$(printf '%s' "$VERSION_INPUT" | tr '[:upper:]' '[:lower:]')
    if [ -z "$input_lc" ] || [ "$input_lc" = latest ]; then
        VERSION=$(resolve_latest)
    else
        VERSION=$(normalise_version "$VERSION_INPUT")
    fi

    TAG="v$VERSION"
    ASSET_VERSION=${VERSION//+/.}
    case "$OS" in
        windows) EXT=zip ;;
        *)       EXT=tar.gz ;;
    esac
    ASSET="IRIS-cli-${ASSET_OS}-${ASSET_ARCH}-${ASSET_VERSION}.${EXT}"
    URL="https://github.com/${REPO}/releases/download/${TAG//+/%2B}/${ASSET}"

    log "downloading IRIS ${VERSION} for ${OS}-${ARCH}"
    curl -fsSL --retry 3 --retry-delay 2 -o "$WORK/$ASSET" "$URL" \
        || die "could not download $URL"
    mkdir -p "$WORK/unpack"
    case "$ASSET" in
        *.tar.gz) tar -xzf "$WORK/$ASSET" -C "$WORK/unpack" ;;
        *.zip)
            if command -v unzip >/dev/null 2>&1; then
                unzip -q -o "$WORK/$ASSET" -d "$WORK/unpack"
            elif command -v powershell.exe >/dev/null 2>&1; then
                powershell.exe -NoProfile -NonInteractive -Command \
                    "Expand-Archive -LiteralPath '$(native_path "$WORK/$ASSET")' -DestinationPath '$(native_path "$WORK/unpack")' -Force"
            else
                die "no unzip or powershell.exe to extract $ASSET"
            fi
            ;;
    esac
    install_binaries "$WORK/unpack"
fi

check_libs
if [ "$SMOKE" = 1 ]; then
    smoke_test
fi

if [ -n "${GITHUB_PATH:-}" ]; then
    native_path "$DEST" >> "$GITHUB_PATH"
fi
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    mode=release
    [ "$FROM_SOURCE" = 1 ] && mode=source
    {
        printf '### IRIS %s\n\n' "$VERSION"
        printf 'Installed `iris` and `iris-ci` (%s, %s-%s) to `%s`; both are on `PATH`.\n' \
            "$mode" "$OS" "$ARCH" "$DEST"
    } >> "$GITHUB_STEP_SUMMARY"
fi

printf '\n'
emit version "$VERSION"
emit iris "$(native_path "$IRIS_PATH")"
emit iris-ci "$(native_path "$CI_PATH")"
log "installed to $DEST"
