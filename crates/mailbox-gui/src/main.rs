//! AiliakMail — calm desktop email client (native GUI).

mod anim;
mod app;
mod avatars;
mod brand;
mod brand_logos;
mod busy;
mod calendar;
mod compose;
mod compose_webview;
mod file_pick;
mod icons;
mod layout;
mod logo_dev;
mod mail_reader;
mod mail_webview;
mod notifications;
mod panes;
mod people;
mod provider_logos;
mod settings;
mod sounds;
mod staks;
mod svg_tex;
mod theme;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    // Local OAuth overrides (gitignored `.env`) — never commit real values.
    let _ = dotenvy::dotenv();

    // Linux + XWayland: wry WebKitGTK child views need matching X11 for winit and GDK.
    // If GDK stays on Wayland while the window is X11, wry panics on Display downcast.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("DISPLAY").is_some() {
            // SAFETY: process startup, before gtk/winit init.
            unsafe {
                std::env::set_var("GDK_BACKEND", "x11");
            }
        }
    }

    // Quiet noisy deps:
    // - sctk-adwaita: GNOME button-layout may include "icon", which CSD ignores
    // - html5ever: ammonia mail sanitize hits unimplemented foster-parenting on
    //   malformed tables ("foster parenting not implemented")
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"))
        .add_directive("sctk_adwaita=error".parse().expect("directive"))
        .add_directive("html5ever=error".parse().expect("directive"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
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
        // wry’s WebKitGTK child webview only accepts X11 handles. On Wayland sessions
        // with XWayland (DISPLAY set), prefer X11 so HTML mail renders instead of
        // falling back to plain text with “window handle kind is not supported”.
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(target_os = "linux")]
            {
                if std::env::var_os("DISPLAY").is_some() {
                    use winit::platform::x11::EventLoopBuilderExtX11;
                    builder.with_x11();
                }
            }
            let _ = builder;
        })),
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
