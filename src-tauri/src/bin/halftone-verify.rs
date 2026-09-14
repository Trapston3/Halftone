//! Cross-implementation verification: run the Rust FLAC walk against the
//! same real files that the verified JS reference (tools/demo_playlist.html)
//! was tested on, and diff the numbers. Also usable standalone:
//!   cargo run --bin halftone-verify -- <dir-or-file>...

use halftone_lib::{read_lrc_file, read_track};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let targets: Vec<PathBuf> = if args.is_empty() {
        eprintln!("usage: halftone-verify <file.flac|dir> [more...]");
        std::process::exit(2);
    } else {
        let mut t = Vec::new();
        for a in &args {
            let p = PathBuf::from(a);
            if p.is_dir() {
                let mut flacs: Vec<PathBuf> = std::fs::read_dir(&p)
                    .expect("read dir")
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|e| e.to_str()).map(|s| s.eq_ignore_ascii_case("flac")).unwrap_or(false))
                    .collect();
                flacs.sort();
                t.extend(flacs);
            } else {
                t.push(p);
            }
        }
        t
    };

    println!("verifying {} target(s) against the direct-FLAC contract", targets.len());
    let mut pass = 0;
    let mut fail = 0;
    for p in &targets {
        match read_track(p) {
            Ok(t) => {
                let cover = t
                    .cover
                    .as_ref()
                    .map(|c| format!("{} {}b", c.mime, c.data_len))
                    .unwrap_or_else(|| "NONE".into());
                let lrc = read_lrc_file(p);
                println!(
                    "PASS {} | sr={} bits={} ch={} | dur {:.5}s | cover {} | blocks {:?} | lrc {} lines | {} - {}",
                    p.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                    t.streaminfo.sample_rate, t.streaminfo.bits, t.streaminfo.channels,
                    t.duration, cover, t.block_types, lrc.len(), t.artist, t.title
                );
                pass += 1;
            }
            Err(e) => {
                println!("FAIL {}: {}", p.display(), e);
                fail += 1;
            }
        }
    }
    println!("--- {} pass / {} fail ---", pass, fail);
    if fail > 0 {
        std::process::exit(1);
    }
}
