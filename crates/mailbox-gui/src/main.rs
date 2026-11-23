//! AiliakMail — calm desktop email client (native GUI).

mod app;
mod avatars;
mod brand;
mod brand_logos;
mod calendar;
mod icons;
mod layout;
mod mail_reader;
mod mail_webview;
mod panes;
mod provider_logos;
mod settings;
mod staks;
mod svg_tex;
mod theme;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    // Local OAuth overrides (gitignored `.env`) — never commit real values.
    let _ = dotenvy::dotenv();

    // Quiet sctk-adwaita: GNOME button-layout may include "icon", which CSD ignores.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("info,sctk_adwaita=error")
        }))
        .with_target(false)
        .init();

    // Window/task switcher icon (embedded in the binary as PNG bytes).
    // Dock/launcher icon on Linux still needs FreeDesktop install — see
    // scripts/install-linux-icon.sh (ELF files cannot carry a DE icon alone).
    let icon = eframe::icon_data::from_png_bytes(include_bytes!(
        "../assets/img/ailiak-icon-256.png"
    ))
    .expect("ailiak-icon-256.png");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("AiliakMail")
            // Must match packaging/com.ailiak.AiliakMail.desktop (StartupWMClass / file id).
            .with_app_id("com.ailiak.AiliakMail")
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "com.ailiak.AiliakMail",
        native_options,
        Box::new(|cc| Ok(Box::new(app::MailboxApp::new(cc)?))),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;

    Ok(())
}
