// Spike #10: Apple Translation EN→TH from Rust. Prints status, per-line latency, translations.
#[path = "../runloop.rs"]
mod runloop;

use std::time::Instant;
use translation::prelude::*;

fn main() {
    let corpus = include_str!("../../corpus/lines-en.txt");
    runloop::run_with_main_loop(move || {
        if let Err(e) = run(corpus) {
            println!("ERROR: {e}");
        }
    });
}

fn run(corpus: &str) -> Result<(), Box<dyn std::error::Error>> {
    let availability = LanguageAvailability::new()?;
    let pair = LanguagePair::between("en", "th");
    println!("pair en→th status: {:?}", availability.status_for_language_pair(&pair)?);

    let session = TranslationSession::new(TranslationSessionConfiguration::new("en", "th"))?;
    println!("can_request_downloads: {:?}", session.can_request_downloads());
    println!("is_ready: {:?}", session.is_ready());

    let t = Instant::now();
    match session.prepare_translation() {
        Ok(()) => println!("prepare_translation: ok ({} ms)", t.elapsed().as_millis()),
        Err(e) => println!("prepare_translation: {e}"),
    }

    let mut times = Vec::new();
    for line in corpus.lines().filter(|l| !l.trim().is_empty()) {
        let t = Instant::now();
        match session.translate(line) {
            Ok(r) => {
                let ms = t.elapsed().as_millis();
                times.push(ms);
                println!("{ms:>5} ms | {line} | {}", r.target_text());
            }
            Err(e) => {
                println!("FAIL | {line} | {e}");
                return Ok(());
            }
        }
    }
    report(&mut times);
    Ok(())
}

fn report(times: &mut [u128]) {
    if times.is_empty() {
        return;
    }
    times.sort_unstable();
    let p = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
    println!(
        "lines={} first={} ms median={} ms p90={} ms max={} ms",
        times.len(),
        times[0].max(0),
        p(0.5),
        p(0.9),
        times[times.len() - 1]
    );
}
