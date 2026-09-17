//! Translates a phrase with the model on this computer, outside the app:
//!
//!   cargo run --release --example translate -- <models dir> "im going mid"
//!
//! Prints what the graphs expect, so a mismatch shows up here and not as a
//! silent failure in the middle of a game.

use std::path::PathBuf;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("models dir"));
    let text = args.next().unwrap_or_else(|| "im going mid, cover me on the right".into());
    let source = args.next().unwrap_or_else(|| "eng_Latn".into());
    let target = args.next().unwrap_or_else(|| "rus_Cyrl".into());
    let game = args.next().unwrap_or_else(|| "all".into());

    let (model, dll) = mgi_hark_lib::models::mt_paths(&dir).expect("translation model not downloaded");
    println!("model: {}\nruntime: {}", model.display(), dll.display());
    let started = Instant::now();
    let mut local = mgi_hark_lib::mt::Local::load(&model, &dll, mgi_hark_lib::mt::threads())?;
    println!("loaded in {} ms", started.elapsed().as_millis());
    local.describe();
    match mgi_hark_lib::glossary::Section::build(&source[..2], &game) {
        Some(section) => println!("glossary: {}", section.summary()),
        None => println!("glossary: nothing for {}", &source[..2]),
    }

    for round in 1..=2 {
        let started = Instant::now();
        let out = local.translate(&text, &source, &target, &game)?;
        println!("{round}: {text:?}\n   -> {out:?}  ({} ms)", started.elapsed().as_millis());
    }
    Ok(())
}
