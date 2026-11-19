use qrcode::render::unicode::Dense1x2;
use qrcode::QrCode;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use yokoten::app::{App, Config};
use yokoten::core::gemini::Settings;
use yokoten::{
    card_audio, db, live, obs, runloop, secret, server, settings, translate, translation_api,
};

const PORT: u16 = 8790;
/// Device name of the capture card OBS reads (UGREEN CM630, FR-CAP-13), and
/// of its audio input that audio profiles hear (FR-AUD-01).
/// ponytail: constant until settings own it.
const CARD_DEVICE: &str = "UGREEN 15390";

type BoxError = Box<dyn Error + Send + Sync>;

/// Apple Translation completes its work on the main dispatch queue, so the
/// main thread does nothing but spin the CoreFoundation run loop. Tokio, and
/// with it the whole app, runs on a second thread; when it returns, the run
/// loop stops and `main` exits with its result.
fn main() -> Result<(), BoxError> {
    runloop::run_with_main_loop(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(serve())
    })
}

async fn serve() -> Result<(), BoxError> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    let conn = db::open(&dir.join("yokoten.db"))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let settings_path = dir.join("settings.json");
    let gemini = translate::Gemini::new(
        Settings::from_stored(&settings::read(&settings_path).into()),
        Arc::new(secret::Keychain::gemini()),
        Arc::new(translate::gemini::Http::new()?),
        Some(settings_path.clone()),
    );
    // Same tone, context lines and extra prompt as Gemini (FR-TRN-14).
    let shared = gemini.clone();
    let claude = translate::Claude::new(
        move || shared.settings(),
        Arc::new(secret::Keychain::anthropic()),
        translate::claude::Http::new()?,
        Some(settings_path.clone()),
    );
    // The `claude` on PATH with the user's own login (FR-TRN-15).
    let shared = gemini.clone();
    let cli = translate::ClaudeCli::new(
        move || shared.settings(),
        Default::default(),
        Some(settings_path.clone()),
    );
    // The `codex` on PATH with the user's own login (FR-TRN-15).
    let shared = gemini.clone();
    let codex = translate::CodexCli::new(
        move || shared.settings(),
        Default::default(),
        Some(settings_path.clone()),
    );
    let app = App::new(
        Config {
            conn,
            frames_dir: path_arg("--frames"),
            obs: Some(obs::Link {
                connect: Arc::new(obs::ws::Local),
                card_device: CARD_DEVICE.into(),
                overlay_url: format!("http://127.0.0.1:{PORT}/overlay"),
            }),
            translators: translators(&gemini, &claude, &cli, &codex)?,
            frame_interval: Duration::from_millis(250),
            live: Some(live::Link {
                card: Some(Arc::new(card_audio::Cpal::new(CARD_DEVICE))),
                ..live::Link::new(gemini.clone(), path_arg("--audio"))
            }),
        },
        shutdown_rx,
    )?;
    settings::restore(&app, &settings_path);
    tokio::spawn(settings::keep_translator(
        app.state.subscribe(),
        settings_path,
    ));

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT)).await?;
    print_urls(lan_ip());
    // Shows a missing language pack on /setup before anyone presses start,
    // without holding the port while it checks (#94).
    tokio::spawn({
        let app = app.clone();
        async move { app.check_translator().await }
    });

    let mut done = shutdown_tx.subscribe();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            println!("\nกำลังปิด Yokoten…");
            let _ = shutdown_tx.send(true);
        }
    });
    let served = axum::serve(
        listener,
        server::router_with(
            app.clone(),
            translation_api::routes(app.clone(), gemini)
                .merge(translation_api::claude_routes(app.clone(), claude))
                .merge(translation_api::claude_cli_routes(app.clone(), cli.clone()))
                .merge(translation_api::codex_cli_routes(app, codex.clone())),
        )
        .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = done.wait_for(|stop| *stop).await;
    })
    .await;
    // No `claude` or `codex` process outlives the app (FR-TRN-15).
    translate::Translator::stop(&cli);
    translate::Translator::stop(&codex);
    served?;
    println!("ปิดแล้ว");
    Ok(())
}

/// Engines to offer; the first is the default (apple when built with it).
fn translators(
    gemini: &translate::Gemini,
    claude: &translate::Claude,
    cli: &translate::ClaudeCli,
    codex: &translate::CodexCli,
) -> Result<Vec<Arc<dyn translate::Translator>>, BoxError> {
    #[allow(unused_mut)]
    let mut list: Vec<Arc<dyn translate::Translator>> = vec![
        Arc::new(gemini.clone()),
        Arc::new(claude.clone()),
        Arc::new(cli.clone()),
        Arc::new(codex.clone()),
        Arc::new(translate::Fake),
    ];
    #[cfg(feature = "apple-translation")]
    list.insert(0, Arc::new(translate::Apple::start()?));
    Ok(list)
}

/// The path after `flag`. `--frames <dir>`: read images from a folder
/// instead of OBS. `--audio <file.wav>`: audio profiles hear a 16 kHz mono
/// WAV file instead of the card. Both are for dev and tests.
fn path_arg(flag: &str) -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == flag {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

fn data_dir() -> Result<PathBuf, BoxError> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join("Library/Application Support/yokoten"))
}

/// IP of the interface that routes outward. UDP connect sends no packet.
fn lan_ip() -> IpAddr {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|s| s.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).map(|_| s))
        .and_then(|s| s.local_addr())
        .map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |a| a.ip())
}

fn print_urls(ip: IpAddr) {
    let reader = format!("http://{ip}:{PORT}/reader");
    println!("Yokoten พร้อมแล้ว");
    println!("  /reader   {reader}   (มือถือใน Wi-Fi เดียวกัน)");
    println!("  /overlay  http://127.0.0.1:{PORT}/overlay   (Browser Source ใน OBS)");
    println!("  /setup    http://127.0.0.1:{PORT}/setup   (เปิดบน Mac เครื่องนี้เท่านั้น)");
    if let Ok(code) = QrCode::new(reader.as_bytes()) {
        println!(
            "\nสแกนเพื่อเปิด /reader\n{}",
            code.render::<Dense1x2>().quiet_zone(true).build()
        );
    }
    println!("กด Ctrl-C เพื่อปิด");
}
