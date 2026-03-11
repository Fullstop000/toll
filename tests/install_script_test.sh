#!/bin/sh

set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
INSTALL_SCRIPT="$ROOT_DIR/install.sh"

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    exit 1
}

assert_file_exists() {
    if [ ! -f "$1" ]; then
        fail "expected file to exist: $1"
    fi
}

assert_file_contains() {
    if ! grep -F "$2" "$1" >/dev/null 2>&1; then
        fail "expected $1 to contain: $2"
    fi
}

# Creates a disposable test environment with fake system tools on PATH.
make_test_env() {
    TEST_TMPDIR=$(mktemp -d)
    export TEST_TMPDIR
    export TEST_BIN_DIR="$TEST_TMPDIR/bin"
    export TEST_HOME_DIR="$TEST_TMPDIR/home"
    export TEST_INSTALL_DIR="$TEST_TMPDIR/install"
    export TEST_LOG_DIR="$TEST_TMPDIR/logs"
    mkdir -p "$TEST_BIN_DIR" "$TEST_HOME_DIR" "$TEST_INSTALL_DIR" "$TEST_LOG_DIR"
    export HOME="$TEST_HOME_DIR"
    export PATH="$TEST_BIN_DIR:$PATH"
}

cleanup_test_env() {
    if [ -n "${TEST_TMPDIR:-}" ] && [ -d "${TEST_TMPDIR:-}" ]; then
        rm -rf "$TEST_TMPDIR"
    fi
}

write_fake_uname() {
    cat >"$TEST_BIN_DIR/uname" <<'EOF'
#!/bin/sh
case "${1:-}" in
    -s) printf '%s\n' "${FAKE_UNAME_S:-Linux}" ;;
    -m) printf '%s\n' "${FAKE_UNAME_M:-x86_64}" ;;
    *) printf '%s\n' "${FAKE_UNAME_S:-Linux}" ;;
esac
EOF
    chmod +x "$TEST_BIN_DIR/uname"
}

write_fake_tar() {
    cat >"$TEST_BIN_DIR/tar" <<'EOF'
#!/bin/sh
set -eu

DEST=""
while [ "$#" -gt 0 ]; do
    if [ "$1" = "-C" ]; then
        shift
        DEST="$1"
    fi
    shift
done

if [ -z "$DEST" ]; then
    exit 1
fi

cp "${FAKE_BINARY_SOURCE:?}" "$DEST/toll"
EOF
    chmod +x "$TEST_BIN_DIR/tar"
}

write_fake_curl_for_release_success() {
    cat >"$TEST_BIN_DIR/curl" <<'EOF'
#!/bin/sh
set -eu

OUTPUT=""
URL=""
PREV=""
for ARG in "$@"; do
    if [ "$PREV" = "-o" ]; then
        OUTPUT="$ARG"
    fi
    case "$ARG" in
        http://*|https://*) URL="$ARG" ;;
    esac
    PREV="$ARG"
done

case "$URL" in
    *"/releases/latest")
        printf '{"tag_name":"v1.2.3"}\n'
        ;;
    *"toll-x86_64-unknown-linux-musl.tar.gz")
        : "${OUTPUT:?missing output}"
        printf 'archive' >"$OUTPUT"
        ;;
    *)
        printf 'unexpected curl url: %s\n' "$URL" >&2
        exit 1
        ;;
esac
EOF
    chmod +x "$TEST_BIN_DIR/curl"
}

write_fake_curl_for_cargo_fallback() {
    cat >"$TEST_BIN_DIR/curl" <<'EOF'
#!/bin/sh
set -eu

OUTPUT=""
URL=""
PREV=""
for ARG in "$@"; do
    if [ "$PREV" = "-o" ]; then
        OUTPUT="$ARG"
    fi
    case "$ARG" in
        http://*|https://*) URL="$ARG" ;;
    esac
    PREV="$ARG"
done

case "$URL" in
    *"/releases/latest")
        printf '{"tag_name":"v9.9.9"}\n'
        ;;
    *".tar.gz")
        : "${OUTPUT:?missing output}"
        exit 22
        ;;
    *)
        printf 'unexpected curl url: %s\n' "$URL" >&2
        exit 1
        ;;
esac
EOF
    chmod +x "$TEST_BIN_DIR/curl"
}

write_fake_cargo() {
    cat >"$TEST_BIN_DIR/cargo" <<'EOF'
#!/bin/sh
set -eu

printf '%s\n' "$*" >"${TEST_LOG_DIR:?}/cargo-args.txt"

ROOT=""
while [ "$#" -gt 0 ]; do
    if [ "$1" = "--root" ]; then
        shift
        ROOT="$1"
    fi
    shift
done

mkdir -p "$ROOT/bin"
cp "${FAKE_BINARY_SOURCE:?}" "$ROOT/bin/toll"
EOF
    chmod +x "$TEST_BIN_DIR/cargo"
}

write_fake_binary() {
    FAKE_BINARY_SOURCE="$TEST_TMPDIR/fake-toll"
    export FAKE_BINARY_SOURCE
    cat >"$FAKE_BINARY_SOURCE" <<'EOF'
#!/bin/sh
printf 'toll 9.9.9\n'
EOF
    chmod +x "$FAKE_BINARY_SOURCE"
}

run_release_install_test() {
    make_test_env
    trap cleanup_test_env EXIT INT TERM
    write_fake_binary
    write_fake_uname
    write_fake_tar
    write_fake_curl_for_release_success

    TOLL_INSTALL_DIR="$TEST_INSTALL_DIR" sh "$INSTALL_SCRIPT" >"$TEST_LOG_DIR/output.txt" 2>&1

    assert_file_exists "$TEST_INSTALL_DIR/toll"
    assert_file_contains "$TEST_LOG_DIR/output.txt" "Installation complete"
    if [ -f "$TEST_LOG_DIR/cargo-args.txt" ]; then
        fail "cargo should not be used when release download succeeds"
    fi

    cleanup_test_env
    trap - EXIT INT TERM
}

run_cargo_fallback_test() {
    make_test_env
    trap cleanup_test_env EXIT INT TERM
    write_fake_binary
    write_fake_uname
    write_fake_tar
    write_fake_curl_for_cargo_fallback
    write_fake_cargo

    TOLL_INSTALL_DIR="$TEST_INSTALL_DIR" sh "$INSTALL_SCRIPT" >"$TEST_LOG_DIR/output.txt" 2>&1

    assert_file_exists "$TEST_INSTALL_DIR/toll"
    assert_file_contains "$TEST_LOG_DIR/output.txt" "Falling back to cargo install"
    assert_file_contains "$TEST_LOG_DIR/cargo-args.txt" "install toll --locked --root "

    cleanup_test_env
    trap - EXIT INT TERM
}

assert_file_exists "$INSTALL_SCRIPT"
run_release_install_test
run_cargo_fallback_test

printf 'install script tests passed\n'
