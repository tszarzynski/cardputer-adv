# lints and a build for the ESP32-S3 (needs the esp toolchain: `espup install`)
default: check

# formatting, and clippy (pedantic and nursery; warnings are errors)
check:
    . ~/export-esp.sh && cargo fmt -- --check && cargo clippy --release -- -D warnings

# format the code
fix:
    cargo fmt

# build the crate
build:
    . ~/export-esp.sh && cargo build --release

# the API docs, in target/xtensa-esp32s3-none-elf/doc/ (docs.rs can't build for Xtensa)
doc:
    . ~/export-esp.sh && cargo doc --no-deps

# what would go to crates.io, checked by building it (publishing itself is `cargo publish`)
package:
    . ~/export-esp.sh && cargo package
