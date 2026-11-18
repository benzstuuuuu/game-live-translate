//! Real Apple Translation, English → Thai. Needs the language pack, so it is
//! ignored like `#[ignore]`: run it with
//! `cargo test --test apple_translation -- --ignored`.
//!
//! No libtest harness (`harness = false` in Cargo.toml): libtest runs every
//! test on a spawned thread, but Translation.framework needs the main thread
//! spinning its run loop, the way `main` runs the app.

use yokoten::core::thai;
use yokoten::runloop;
use yokoten::translate::{Apple, Translator};

const NAME: &str = "apple_translates_a_sample_sentence_to_thai";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        println!("{NAME}: test");
        return;
    }
    let run_ignored = args
        .iter()
        .any(|a| a == "--ignored" || a == "--include-ignored");
    let filters: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let selected = filters.is_empty() || filters.iter().any(|f| NAME.contains(f.as_str()));
    if !selected {
        println!("\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out\n");
        return;
    }
    if !run_ignored {
        println!("\nrunning 1 test\ntest {NAME} ... ignored\n\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n");
        return;
    }
    println!("\nrunning 1 test");
    let result = runloop::run_with_main_loop(|| {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("tokio runtime")
            .block_on(apple_translates_a_sample_sentence_to_thai())
    });
    match result {
        Ok(()) => println!("test {NAME} ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"),
        Err(e) => {
            println!("test {NAME} ... FAILED\n{e}\n\ntest result: FAILED. 0 passed; 1 failed\n");
            std::process::exit(101);
        }
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
