//! Generates the offline vanilla server SHA-1 lookup table from Mojang metadata.

use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufWriter, Write},
    path::PathBuf,
};

const LEGACY_SERVER_SHA1_PATH: &str = "data/legacy_server_sha1.json";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={LEGACY_SERVER_SHA1_PATH}");
    let versions: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(LEGACY_SERVER_SHA1_PATH)?)?;
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let mut output = BufWriter::new(fs::File::create(out_dir.join("legacy_server_sha1.rs"))?);
    writeln!(
        output,
        "static LEGACY_SERVER_SHA1: &[([u8; 20], &str)] = &["
    )?;
    // Lowercase hex and decoded bytes sort identically, so the slice is an implicit binary search tree.
    for (sha1, version) in versions {
        if sha1.len() != 40 || !sha1.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{LEGACY_SERVER_SHA1_PATH}: invalid SHA-1 key {sha1:?}"),
            )
            .into());
        }
        let hash: [u8; 20] = std::array::from_fn(|i| {
            u8::from_str_radix(&sha1[2 * i..2 * i + 2], 16).expect("validated hex pair")
        });
        writeln!(output, "    ({hash:?}, {version:?}),")?;
    }
    writeln!(output, "];")?;
    output.flush()?;
    Ok(())
}
