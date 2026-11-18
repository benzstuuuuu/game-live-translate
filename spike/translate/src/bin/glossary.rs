// Spike #27: can a glossary survive Apple Translation EN→TH?
// For each line with a glossary term, compare:
//   raw  — translate as-is, then swap any English term left over (FR-GLS-05 as written)
//   ph:* — swap each term for a placeholder before translating, put the Thai term back after
// A placeholder strategy "holds" on a line when every placeholder comes back exactly once.
#[path = "../runloop.rs"]
mod runloop;

use translation::prelude::*;

const GLOSSARY: &[(&str, &str)] = &[
    ("Ember Crown", "มงกุฎเอมเบอร์"),
    ("Hollow Order", "ภาคีฮอลโลว์"),
    ("Captain Rhee", "กัปตันรี"),
    ("Warden", "วอร์เดน"),
    ("Paimon", "ไพมอน"),
    ("Mondstadt", "มอนด์สตัดท์"),
];

const LINES: &[&str] = &[
    "The Ember Crown was stolen from the vault three days ago.",
    "Your reputation precedes you, Warden.",
    "The Hollow Order doesn't forgive deserters.",
    "Captain Rhee wants a word with you. Now.",
    "The Ember Crown belongs to the people, not to the throne.",
    "The Warden will not bargain with thieves.",
    "The Hollow Order has eyes in every port.",
    "Captain Rhee was right about you.",
    "Welcome back, Warden. We kept your seat warm.",
    "Paimon is hungry again!",
    "Let's head back to Mondstadt before the storm.",
    "Did the Warden really steal the Ember Crown from the Hollow Order?",
    "Tell Captain Rhee that Paimon found the Ember Crown.",
    "Warden, the Hollow Order is waiting for you in Mondstadt.",
    "Nobody trusts the Hollow Order anymore.",
    "Is that the Ember Crown? It's smaller than I imagined.",
    "The Warden's sword is broken.",
    "Paimon's map says Mondstadt is close.",
    "Ember Crown, Ember Crown, always the Ember Crown!",
    "We met the Warden. The Warden smiled.",
];

// (name, placeholder for term index i)
fn styles() -> Vec<(&'static str, fn(usize) -> String)> {
    vec![
        ("ph:Zx", |i| format!("Zx{}q", i + 1)),
        ("ph:[n]", |i| format!("[{}]", i + 1)),
        ("ph:#n", |i| format!("#{}", i + 1)),
        ("ph:name", |i| ["Kovar", "Daleth", "Imrik", "Torvu", "Selka", "Brann"][i].to_string()),
    ]
}

fn main() {
    runloop::run_with_main_loop(|| {
        if let Err(e) = run() {
            println!("ERROR: {e}");
        }
    });
}

fn fix_am(s: &str) -> String {
    s.replace("\u{0E4D}\u{0E32}", "\u{0E33}")
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let session = TranslationSession::new(TranslationSessionConfiguration::new("en-GB", "th"))?;
    session.prepare_translation()?;
    let tr = |s: &str| -> Result<String, Box<dyn std::error::Error>> { Ok(fix_am(session.translate(s)?.target_text())) };

    let mut held = vec![0usize; styles().len()];
    let mut raw_ok = 0;
    for line in LINES {
        let used: Vec<usize> = (0..GLOSSARY.len()).filter(|&i| line.contains(GLOSSARY[i].0)).collect();
        println!("\nEN   | {line}");

        // raw + post-replace of English leftovers
        let raw = tr(line)?;
        let mut fixed = raw.clone();
        for &i in &used {
            fixed = fixed.replace(GLOSSARY[i].0, GLOSSARY[i].1);
        }
        let ok = used.iter().all(|&i| fixed.contains(GLOSSARY[i].1));
        raw_ok += ok as usize;
        println!("raw  | {raw}");
        println!("  {} | {fixed}", if ok { "OK" } else { "--" });

        for (k, (name, ph)) in styles().into_iter().enumerate() {
            let mut src = line.to_string();
            for &i in &used {
                src = src.replace(GLOSSARY[i].0, &ph(i));
            }
            let out = tr(&src)?;
            let ok = used.iter().all(|&i| out.matches(&ph(i)).count() == src.matches(&ph(i)).count());
            let mut back = out.clone();
            for &i in &used {
                back = back.replace(&ph(i), GLOSSARY[i].1);
            }
            held[k] += ok as usize;
            println!("{name:<8}| {out}\n  {} | {back}", if ok { "OK" } else { "--" });
        }
    }
    println!("\n== {} lines with glossary terms", LINES.len());
    println!("raw+post-replace: {raw_ok}/{}", LINES.len());
    for (k, (name, _)) in styles().into_iter().enumerate() {
        println!("{name}: {}/{}", held[k], LINES.len());
    }
    Ok(())
}
