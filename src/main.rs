use qrcode::render::unicode::Dense1x2;
use qrcode::QrCode;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use yokoten::app::{App, Config};
use yokoten::{db, obs, server, translate};

const PORT: u16 = 8790;
/// Device name of the capture card OBS reads (UGREEN CM630, FR-CAP-13).
/// ponytail: constant until settings own it.
const CARD_DEVICE: &str = "UGREEN 15390";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    let conn = db::open(&dir.join("yokoten.db"))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let app = App::new(
        Config {
            conn,
            frames_dir: frames_arg(),
            obs: Some(obs::Link {
                connect: Arc::new(obs::ws::Local),
                card_device: CARD_DEVICE.into(),
            }),
            // ponytail: fake until Apple Translation lands (#35)
            translator: Arc::new(translate::Fake),
            frame_interval: Duration::from_millis(250),
        },
        shutdown_rx,
    )?;

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT)).await?;
    print_urls(lan_ip());

    let mut done = shutdown_tx.subscribe();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            println!("\nกำลังปิด Yokoten…");
            let _ = shutdown_tx.send(true);
        }
    });
    axum::serve(
        listener,
        server::router(app).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = done.wait_for(|stop| *stop).await;
    })
    .await?;
    println!("ปิดแล้ว");
    Ok(())
}

/// `--frames <dir>`: read images from a folder instead of OBS (dev and tests).
/// Without it frames come from OBS.
fn frames_arg() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--frames" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

fn data_dir() -> Result<PathBuf, Box<dyn Error>> {
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
