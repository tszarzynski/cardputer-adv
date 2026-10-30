# lints for the ESP32-S3 (needs the esp toolchain: `espup install`), and the host tests
default: check test

# The esp toolchain's environment, from espup's export file where there is one
# (CI's toolchain action sets it up itself).
esp := "if [ -f ~/export-esp.sh ]; then . ~/export-esp.sh; fi"

# formatting, and clippy (pedantic and nursery; warnings are errors), with no features and with all of them, and the example; then the README's examples
check: && readme
    {{esp}} && cargo fmt -- --check && cargo clippy --release -- -D warnings && cargo clippy --release --all-features --example hardware -- -D warnings && cargo clippy --release --example flush -- -D warnings && cargo clippy --release --example hello -- -D warnings

# the README's examples and the crate docs' quick start, compiled (readme-check/ takes them out of their files), with fmt and clippy on its own code
readme:
    {{esp}} && cd readme-check && cargo fmt -- --check && cargo clippy --target-dir ../target -- -D warnings

# the pure logic's tests, on the host with stable Rust (host-tests/ builds those modules from src/), with fmt and clippy on them
test:
    cd host-tests && cargo fmt -- --check && cargo clippy --all-targets -- -D warnings && cargo test

# format the code
fix:
    cargo fmt

# build the crate, the hardware test, the flush timing and the hello example
build:
    {{esp}} && cargo build --release && cargo build --release --all-features --example hardware && cargo build --release --example flush && cargo build --release --example hello

# try each part on a Cardputer and follow its log (examples/hardware.rs says what to look for)
hardware:
    {{esp}} && cargo run --release --features sd-card --example hardware

# time FrameDisplay::flush on a Cardputer for changed areas of different shapes, and log it (examples/flush.rs)
flush:
    {{esp}} && cargo run --release --example flush

# the API docs, with all the features, in target/xtensa-esp32s3-none-elf/doc/ (docs.rs can't build for Xtensa; CI publishes them to GitHub Pages)
# The flags go to this crate's rustdoc only: with RUSTDOCFLAGS set, xtensa-lx-rt's build script stops checking the target and sets up the plain ESP32, which later builds reuse.
doc:
    {{esp}} && cargo rustdoc --lib --all-features -- --cfg docsrs -D warnings

# what would go to crates.io, checked by building it (publishing itself is `cargo publish`)
package:
    {{esp}} && cargo package

# the public API against the last release on crates.io, by cargo-semver-checks (`cargo install cargo-semver-checks`): a breaking change needs a new major version (in 0.x, a minor one)
# cargo-semver-checks can't build for Xtensa itself, so the rustdoc JSON is built here; the release is unpacked under target/, where it builds with this repo's toolchain and config.
semver:
    #!/usr/bin/env bash
    set -euo pipefail
    {{esp}}
    out=target/semver
    release=$(curl -sf -A "cardputer-adv semver check" https://crates.io/api/v1/crates/cardputer-adv \
        | python3 -c 'import json, sys; print(json.load(sys.stdin)["crate"]["max_stable_version"])')
    rm -rf "$out/release" && mkdir -p "$out/release"
    curl -sfL "https://static.crates.io/crates/cardputer-adv/cardputer-adv-$release.crate" \
        | tar -xz -C "$out/release" --strip-components 1
    json() {
        RUSTC_BOOTSTRAP=1 cargo rustdoc --manifest-path "$1/Cargo.toml" --lib --all-features --target-dir "$2" \
            -- -Z unstable-options --output-format json
    }
    json "$out/release" "$out/release-target"
    json . "$out/current-target"
    cargo semver-checks \
        --baseline-rustdoc "$out/release-target/xtensa-esp32s3-none-elf/doc/cardputer_adv.json" \
        --current-rustdoc "$out/current-target/xtensa-esp32s3-none-elf/doc/cardputer_adv.json"
