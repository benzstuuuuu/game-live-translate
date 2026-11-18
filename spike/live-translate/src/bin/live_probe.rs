//! Stream 16 kHz s16le mono PCM from stdin into gemini-3.5-live-translate-preview and log what comes back.
//! usage: ffmpeg -re ... -ar 16000 -ac 1 -f s16le - | live_probe
//! env:   GEMINI_API_KEY (required) · MODALITY=AUDIO|TEXT · COMPRESS=1 · RESUME=1 · SYS="<system instruction>" · DROP_AT=<secs>
//! prints `t \t kind \t detail` (t = seconds since the first chunk was sent), writes Thai audio to out/th_24k.pcm.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::io::Write;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

const URL: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
const MODEL: &str = "models/gemini-3.5-live-translate-preview";
const CHUNK: usize = 3200; // 100 ms of 16 kHz s16le mono

fn env_on(k: &str) -> bool {
    std::env::var(k).is_ok_and(|v| v == "1")
}

fn setup(handle: Option<&str>) -> Value {
    let modality = std::env::var("MODALITY").unwrap_or_else(|_| "AUDIO".into());
    let mut s = json!({
        "model": MODEL,
        "inputAudioTranscription": {},
        "outputAudioTranscription": {},
        "generationConfig": {
            "responseModalities": [modality],
            "translationConfig": { "targetLanguageCode": "th", "echoTargetLanguage": false }
        }
    });
    if let Ok(sys) = std::env::var("SYS") {
        s["systemInstruction"] = json!({ "parts": [{ "text": sys }] });
    }
    if env_on("COMPRESS") {
        s["contextWindowCompression"] = json!({ "slidingWindow": {} });
    }
    if env_on("RESUME") || handle.is_some() {
        s["sessionResumption"] = handle.map_or(json!({}), |h| json!({ "handle": h }));
    }
    json!({ "setup": s })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("GEMINI_API_KEY").map_err(|_| "GEMINI_API_KEY not set")?;
    std::fs::create_dir_all("out")?;
    let mut th_audio = std::fs::File::create("out/th_24k.pcm")?;

    // stdin -> 100 ms chunks; None marks EOF
    let (tx, mut rx) = mpsc::channel::<Option<Vec<u8>>>(64);
    tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        loop {
            let mut buf = vec![0u8; CHUNK];
            match stdin.read_exact(&mut buf).await {
                Ok(_) => {
                    if tx.send(Some(buf)).await.is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(None).await;
                    break;
                }
            }
        }
    });

    let mut t0: Option<Instant> = None;
    let t = |t0: &Option<Instant>| t0.map_or(0.0, |s| s.elapsed().as_secs_f64());
    let mut handle: Option<String> = None;
    let mut sent_secs = 0.0f64;
    let mut eof_at: Option<Instant> = None;
    let mut audio_bytes_turn = 0usize;
    let mut connects = 0;
    let drop_at: Option<f64> = std::env::var("DROP_AT").ok().and_then(|v| v.parse().ok());

    'session: loop {
        connects += 1;
        let mut req = URL.into_client_request()?;
        req.headers_mut().insert("x-goog-api-key", key.parse()?);
        let (mut ws, _) = tokio_tungstenite::connect_async(req).await?;
        ws.send(Message::text(setup(handle.as_deref()).to_string())).await?;
        println!("{:.2}\tconnect\t#{connects} resume={}", t(&t0), handle.is_some());

        let mut ready = false;
        loop {
            // after EOF keep listening 10 s for late output, then stop
            if eof_at.is_some_and(|e| e.elapsed().as_secs() >= 10) {
                let _ = ws.close(None).await;
                break 'session;
            }
            // DROP_AT=<secs>: close the first connection mid-stream to test resumption
            if connects == 1 && drop_at.is_some_and(|d| t(&t0) >= d) {
                let _ = ws.close(None).await;
                println!("{:.2}\tdrop\tforced", t(&t0));
                break;
            }
            tokio::select! {
                chunk = rx.recv(), if ready && eof_at.is_none() => match chunk.flatten() {
                    Some(buf) => {
                        t0.get_or_insert_with(Instant::now);
                        sent_secs += buf.len() as f64 / 32000.0;
                        let msg = json!({ "realtimeInput": { "audio": { "data": B64.encode(&buf), "mimeType": "audio/pcm;rate=16000" } } });
                        ws.send(Message::text(msg.to_string())).await?;
                    }
                    None => {
                        eof_at = Some(Instant::now());
                        ws.send(Message::text(json!({ "realtimeInput": { "audioStreamEnd": true } }).to_string())).await?;
                        println!("{:.2}\teof\tsent {sent_secs:.1}s of audio", t(&t0));
                    }
                },
                _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {}
                msg = ws.next() => {
                    let text = match msg {
                        Some(Ok(Message::Text(s))) => s.to_string(),
                        Some(Ok(Message::Binary(b))) => String::from_utf8_lossy(&b).into_owned(),
                        Some(Ok(Message::Close(f))) => {
                            println!("{:.2}\tclose\t{:?}", t(&t0), f);
                            break;
                        }
                        Some(Ok(_)) => continue,
                        Some(Err(e)) => {
                            println!("{:.2}\terror\t{e}", t(&t0));
                            break;
                        }
                        None => {
                            println!("{:.2}\tclose\tstream ended", t(&t0));
                            break;
                        }
                    };
                    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    let now = t(&t0);
                    if v.get("setupComplete").is_some() {
                        ready = true;
                        println!("{now:.2}\tready\t");
                    }
                    if let Some(sc) = v.get("serverContent") {
                        for (k, label) in [("inputTranscription", "en"), ("outputTranscription", "th")] {
                            if let Some(tr) = sc.get(k) {
                                let mut extra = tr.clone();
                                extra.as_object_mut().map(|o| o.remove("text"));
                                // m = seconds of audio sent so far (media time), the clock latency is measured against
                                println!("{now:.2}\t{label}\t{}\t{}\tm={sent_secs:.2}", tr["text"].as_str().unwrap_or(""), extra);
                            }
                        }
                        for p in sc["modelTurn"]["parts"].as_array().into_iter().flatten() {
                            if let Some(d) = p["inlineData"]["data"].as_str() {
                                let pcm = B64.decode(d)?;
                                if audio_bytes_turn == 0 {
                                    println!("{now:.2}\taudio\tfirst chunk {} {}", pcm.len(), p["inlineData"]["mimeType"]);
                                }
                                audio_bytes_turn += pcm.len();
                                th_audio.write_all(&pcm)?;
                            }
                            if let Some(s) = p["text"].as_str() {
                                println!("{now:.2}\ttext\t{s}");
                            }
                        }
                        for flag in ["turnComplete", "generationComplete", "interrupted"] {
                            if sc.get(flag).is_some_and(|b| b == true) {
                                println!("{now:.2}\t{flag}\taudio {:.2}s", audio_bytes_turn as f64 / 48000.0);
                                audio_bytes_turn = 0;
                            }
                        }
                    }
                    if let Some(u) = v.get("usageMetadata") {
                        println!("{now:.2}\tusage\t{u}");
                    }
                    if let Some(g) = v.get("goAway") {
                        println!("{now:.2}\tgoAway\t{g}");
                    }
                    if let Some(r) = v.get("sessionResumptionUpdate") {
                        if let Some(h) = r["newHandle"].as_str() {
                            handle = Some(h.to_string());
                        }
                        println!("{now:.2}\tresume\tresumable={} handle={}", r["resumable"], r["newHandle"].is_string());
                    }
                    let known = ["setupComplete", "serverContent", "usageMetadata", "goAway", "sessionResumptionUpdate"];
                    let other: Vec<_> = v.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).filter(|k| !known.contains(k)).collect();
                    if !other.is_empty() {
                        println!("{now:.2}\tother\t{}", text.chars().take(400).collect::<String>());
                    }
                }
            }
        }
        // connection dropped: reconnect (with the last handle) unless input is done
        if eof_at.is_some() || connects >= 20 {
            break;
        }
    }
    println!("{:.2}\tdone\tsent {sent_secs:.1}s, connects {connects}", t(&t0));
    Ok(())
}
