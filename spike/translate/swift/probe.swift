// Spike #10 fallback probe: same questions as the Rust bins, straight against the frameworks,
// because the Rust crates need SDK 26.5 and this machine has CLT 26.0.
// Run: swift spike/translate/swift/probe.swift spike/translate/corpus/lines-en.txt
import Foundation
import FoundationModels
import Translation

let lines = try String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8)
    .split(separator: "\n").map(String.init).filter { !$0.isEmpty }

func ms(_ start: ContinuousClock.Instant) -> Int {
    let d = ContinuousClock.now - start
    return Int(d.components.seconds * 1000 + d.components.attoseconds / 1_000_000_000_000_000)
}

// --- Apple Translation EN→TH, no UI ---
let en = Locale.Language(identifier: "en-GB"), th = Locale.Language(identifier: "th")
let status = await LanguageAvailability().status(from: en, to: th)
print("translation en→th status: \(status)")
let session = TranslationSession(installedSource: en, target: th)
var times: [Int] = []
do {
    for line in lines {
        let t = ContinuousClock.now
        let r = try await session.translate(line)
        let m = ms(t); times.append(m)
        print(String(format: "%5d ms | %@ | %@", m, line, r.targetText))
    }
} catch {
    print("translation FAIL: \(error)")
}
if !times.isEmpty {
    let s = times.sorted()
    print("translation lines=\(s.count) first=\(times[0]) ms median=\(s[s.count / 2]) ms p90=\(s[Int(Double(s.count - 1) * 0.9)]) ms max=\(s.last!) ms")
}

// --- Foundation Models ---
let model = SystemLanguageModel.default
print("afm availability: \(model.availability)")
for id in ["th", "th-TH", "en-US"] {
    print("afm supportsLocale(\(id)): \(model.supportsLocale(Locale(identifier: id)))")
}
print("afm supportedLanguages: \(model.supportedLanguages.map { $0.minimalIdentifier }.sorted().joined(separator: ", "))")
if case .available = model.availability {
    let afm = LanguageModelSession(instructions: """
        You translate English video game subtitles into natural spoken Thai. Reply with the Thai \
        translation only. Always use: Ember Crown = มงกุฎอัคคี; Warden = ผู้พิทักษ์; Hollow Order = ภาคีกลวง.
        """)
    for line in lines.prefix(10) {
        let t = ContinuousClock.now
        do {
            let r = try await afm.respond(to: line)
            print(String(format: "%5d ms | %@ | %@", ms(t), line, r.content))
        } catch {
            print("afm FAIL | \(line) | \(error)")
            break
        }
    }
} else {
    print("afm SKIP respond: unavailable")
}
