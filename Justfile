# lints and a build for the ESP32-S3 (needs the esp toolchain: `espup install`)
default: check

# The esp toolchain's environment, from espup's export file where there is one
# (CI's toolchain action sets it up itself).
esp := "if [ -f ~/export-esp.sh ]; then . ~/export-esp.sh; fi"

# formatting, and clippy (pedantic and nursery; warnings are errors), with no features and with all of them, and the example
check:
    {{esp}} && cargo fmt -- --check && cargo clippy --release -- -D warnings && cargo clippy --release --all-features --example hardware -- -D warnings

# format the code
fix:
    cargo fmt

# build the crate, and the hardware test
build:
    {{esp}} && cargo build --release && cargo build --release --all-features --example hardware

# try each part on a Cardputer and follow its log (examples/hardware.rs says what to look for)
hardware:
    {{esp}} && cargo run --release --features sd-card --example hardware

# the API docs, with all the features, in target/xtensa-esp32s3-none-elf/doc/ (docs.rs can't build for Xtensa; CI publishes them to GitHub Pages)
# The flags go to this crate's rustdoc only: with RUSTDOCFLAGS set, xtensa-lx-rt's build script stops checking the target and sets up the plain ESP32, which later builds reuse.
doc:
    {{esp}} && cargo rustdoc --lib --all-features -- --cfg docsrs -D warnings

# what would go to crates.io, checked by building it (publishing itself is `cargo publish`)
package:
    {{esp}} && cargo package
