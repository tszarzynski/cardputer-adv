# lints and a build for the ESP32-S3 (needs the esp toolchain: `espup install`)
default: check

# formatting, and clippy (pedantic and nursery; warnings are errors), with and without the features, and the example
check:
    . ~/export-esp.sh && cargo fmt -- --check && cargo clippy --release -- -D warnings && cargo clippy --release --features sd-card --example hardware -- -D warnings

# format the code
fix:
    cargo fmt

# build the crate, and the hardware test
build:
    . ~/export-esp.sh && cargo build --release && cargo build --release --features sd-card --example hardware

# try each part on a Cardputer and follow its log (examples/hardware.rs says what to look for)
hardware:
    . ~/export-esp.sh && cargo run --release --features sd-card --example hardware

# the API docs, in target/xtensa-esp32s3-none-elf/doc/ (docs.rs can't build for Xtensa)
doc:
    . ~/export-esp.sh && cargo doc --no-deps --features sd-card

# what would go to crates.io, checked by building it (publishing itself is `cargo publish`)
package:
    . ~/export-esp.sh && cargo package
