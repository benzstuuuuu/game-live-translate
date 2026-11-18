// Spike #10: Foundation Models on this machine — availability, Thai support, and
// whether "respond in Thai" works or throws unsupportedLanguageOrLocale.
use foundation_models::prelude::*;
use std::time::Instant;

const INSTRUCTIONS: &str = "You translate English video game subtitles into natural spoken Thai. \
Reply with the Thai translation only. Always use these glossary terms: \
Ember Crown = มงกุฎอัคคี; Warden = ผู้พิทักษ์; Hollow Order = ภาคีกลวง.";

fn main() {
    println!("availability: {:?}", SystemLanguageModel::availability());
    match SystemLanguageModel::with_use_case(UseCase::General, Guardrails::Default) {
        Ok(model) => {
            for loc in ["th", "th-TH", "en-US"] {
                println!("supports_locale({loc}): {}", model.supports_locale(loc));
            }
            let langs = model.supported_languages();
            println!("supported_languages ({}): {}", langs.len(), langs.join(", "));
        }
        Err(e) => println!("model: {e}"),
    }
    if !SystemLanguageModel::is_available() {
        println!("SKIP respond: model unavailable on this machine");
        return;
    }
    let corpus = include_str!("../../corpus/lines-en.txt");
    let session = match LanguageModelSession::with_instructions(INSTRUCTIONS) {
        Ok(s) => s,
        Err(e) => return println!("session: {e}"),
    };
    for line in corpus.lines().take(10) {
        let t = Instant::now();
        match session.respond(line) {
            Ok(r) => println!("{:>5} ms | {line} | {r}", t.elapsed().as_millis()),
            Err(e) => return println!("FAIL | {line} | {e}"),
        }
    }
}
