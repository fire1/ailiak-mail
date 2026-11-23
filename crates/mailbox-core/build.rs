//! Load workspace `.env` into compile-time env so `option_env!("AILIAKMAIL_OAUTH_…")` works
//! without committing secrets. CI should set the same vars as secrets.

fn main() {
    // crates/mailbox-core → repo root
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root_env = manifest_dir.join("../../.env");
    let _ = dotenvy::from_path(root_env);
    let _ = dotenvy::dotenv();

    for key in [
        "AILIAKMAIL_OAUTH_GOOGLE_CLIENT_ID",
        "AILIAKMAIL_OAUTH_GOOGLE_CLIENT_SECRET",
        "AILIAKMAIL_OAUTH_MICROSOFT_CLIENT_ID",
        "AILIAKMAIL_OAUTH_YAHOO_CLIENT_ID",
        "AILIAKMAIL_OAUTH_YAHOO_CLIENT_SECRET",
        "AILIAKMAIL_OAUTH_YANDEX_CLIENT_ID",
        "AILIAKMAIL_OAUTH_YANDEX_CLIENT_SECRET",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
        if let Ok(val) = std::env::var(key) {
            let val = val.trim();
            if !val.is_empty() {
                // Make available to option_env! in this crate.
                println!("cargo:rustc-env={key}={val}");
            }
        }
    }
}
