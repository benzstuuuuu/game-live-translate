//! Real Apple Translation, English → Thai. Needs the language pack, so it is
//! ignored like `#[ignore]`: run it with
//! `cargo test --test apple_translation -- --ignored`.
//!
//! No libtest harness (`harness = false` in Cargo.toml): libtest runs every
//! test on a spawned thread, but Translation.framework needs the main thread
//! spinning its run loop, the way `main` runs the app.

#[path = "fixtures/glossary_spike.rs"]
mod spike;

use yokoten::core::{glossary, thai};
use yokoten::runloop;
use yokoten::translate::{Apple, Translator};

const TESTS: &[&str] = &[
    "apple_translates_a_sample_sentence_to_thai",
    "apple_keeps_the_spike_glossary_with_placeholders",
];

async fn run(name: &str) -> Result<(), String> {
    match name {
        "apple_translates_a_sample_sentence_to_thai" => {
            apple_translates_a_sample_sentence_to_thai().await
        }
        _ => apple_keeps_the_spike_glossary_with_placeholders().await,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        TESTS.iter().for_each(|t| println!("{t}: test"));
        return;
    }
    let run_ignored = args
        .iter()
        .any(|a| a == "--ignored" || a == "--include-ignored");
    let filters: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let selected: Vec<&str> = TESTS
        .iter()
        .copied()
        .filter(|t| filters.is_empty() || filters.iter().any(|f| t.contains(f.as_str())))
        .collect();
    let out = TESTS.len() - selected.len();
    println!("\nrunning {} tests", selected.len());
    if !run_ignored {
        selected
            .iter()
            .for_each(|t| println!("test {t} ... ignored"));
        println!(
            "\ntest result: ok. 0 passed; 0 failed; {} ignored; 0 measured; {out} filtered out\n",
            selected.len()
        );
        return;
    }
    let count = selected.len();
    let failed = runloop::run_with_main_loop(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("tokio runtime");
        let mut failed = 0;
        for t in selected {
            match rt.block_on(run(t)) {
                Ok(()) => println!("test {t} ... ok"),
                Err(e) => {
                    failed += 1;
                    println!("test {t} ... FAILED\n{e}");
                }
            }
        }
        failed
    });
    let passed = count - failed;
    if failed == 0 {
        println!("\ntest result: ok. {passed} passed; 0 failed; 0 ignored; 0 measured; {out} filtered out\n");
    } else {
        println!("\ntest result: FAILED. {passed} passed; {failed} failed\n");
        std::process::exit(101);
    }
}

/// #44: the 20 lines of the #27 spike through `translate_with_glossary`; each
/// line must come back with every Thai term, with the placeholders held
/// (no retranslation).
async fn apple_keeps_the_spike_glossary_with_placeholders() -> Result<(), String> {
    let apple = Apple::start().map_err(|e| e.to_string())?;
    apple
        .check()
        .await
        .map_err(|e| format!("{} | {}", e.message, e.fix))?;
    let terms = spike::terms();
    let mut ok = 0;
    for line in spike::LINES {
        let protected = glossary::protect(line, &terms);
        let raw = apple.translate(&protected.text).await?;
        let held = protected.restore(&thai::fix_am(&raw)).is_some();
        let thai_text = thai::fix_am(&apple.translate_with_glossary(line, &[], &terms).await?);
        let missing = spike::missing(line, &thai_text);
        let good = held && missing.is_empty();
        ok += good as usize;
        println!(
            "  {} {line}\n     → {thai_text}",
            if good { "OK" } else { "--" }
        );
    }
    println!("  apple glossary: {ok}/{}", spike::LINES.len());
    if ok == spike::LINES.len() {
        Ok(())
    } else {
        Err(format!(
            "{ok}/{} lines kept the glossary",
            spike::LINES.len()
        ))
    }
}

async fn apple_translates_a_sample_sentence_to_thai() -> Result<(), String> {
    let apple = Apple::start().map_err(|e| e.to_string())?;
    apple
        .check()
        .await
        .map_err(|e| format!("{} | {}", e.message, e.fix))?;

    let source = "Keep your lantern low. The guards are close.";
    let started = std::time::Instant::now();
    let thai_text = thai::fix_am(&apple.translate(source).await?);
    println!(
        "  {source}\n  → {thai_text}  ({} ms)",
        started.elapsed().as_millis()
    );

    let is_thai = |c: char| ('\u{0E00}'..='\u{0E7F}').contains(&c);
    if !thai_text.chars().any(is_thai) {
        return Err(format!("no Thai letters in {thai_text:?}"));
    }
    if thai_text.contains("\u{0E4D}\u{0E32}") {
        return Err("nikhahit + sara aa left after fix_am".into());
    }
    Ok(())
}
