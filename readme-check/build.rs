//! Takes the examples out of the README (its ```rust blocks) and the crate
//! docs (their quick start, a `//! ```ignore` block) for readme.rs to
//! compile: each into `OUT_DIR/<source>_<n>.rs` as one block expression,
//! with `OUT_DIR/count.rs` saying how many each source has.

use std::{env, fs, path::Path};

/// The code blocks in `text` that open with `fence`, each line with
/// `prefix` taken off.
fn blocks(text: &str, prefix: &str, fence: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut block: Option<String> = None;
    for line in text.lines().filter_map(|line| line.strip_prefix(prefix)) {
        let line = line.strip_prefix(' ').unwrap_or(line);
        match &mut block {
            None if line == fence => block = Some(String::new()),
            Some(code) if line == "```" => {
                blocks.push(format!("{{\n{code}}}\n"));
                block = None;
            }
            Some(code) => {
                code.push_str(line);
                code.push('\n');
            }
            None => {}
        }
    }
    blocks
}

fn main() {
    let out = env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let sources = [
        ("readme", "../README.md", "", "```rust"),
        ("crate_docs", "../src/lib.rs", "//!", "```ignore"),
    ];
    let mut count = String::new();
    for (name, path, prefix, fence) in sources {
        println!("cargo::rerun-if-changed={path}");
        let text = fs::read_to_string(path).expect("the crate's sources are next door");
        let blocks = blocks(&text, prefix, fence);
        for (index, block) in blocks.iter().enumerate() {
            let file = Path::new(&out).join(format!("{name}_{}.rs", index + 1));
            fs::write(file, block).expect("OUT_DIR is writable");
        }
        count.push_str(&format!(
            "const {}: usize = {};\n",
            name.to_uppercase(),
            blocks.len()
        ));
    }
    fs::write(Path::new(&out).join("count.rs"), count).expect("OUT_DIR is writable");
}
