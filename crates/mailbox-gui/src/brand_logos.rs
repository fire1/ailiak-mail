//! Brand marks for Staks (sender/domain stacks) — SVG → 64×64 textures.

use egui::{ColorImage, Context, Id, TextureHandle, TextureOptions};

use crate::svg_tex;

const LOGO_PX: u32 = 64;

/// On-screen size in the Staks rail / list avatars.
pub const DISPLAY_SIZE: f32 = 18.0;
pub const LIST_SIZE: f32 = 26.0;

#[derive(Clone)]
struct BrandPack {
    google: TextureHandle,
    instagram: TextureHandle,
    linkedin: TextureHandle,
    facebook: TextureHandle,
    meta: TextureHandle,
    x: TextureHandle,
    github: TextureHandle,
    gitlab: TextureHandle,
    discord: TextureHandle,
    microsoft: TextureHandle,
    apple: TextureHandle,
    amazon: TextureHandle,
    stripe: TextureHandle,
    notion: TextureHandle,
    slack: TextureHandle,
    dropbox: TextureHandle,
    figma: TextureHandle,
    svgator: TextureHandle,
    cursor: TextureHandle,
    warp: TextureHandle,
    abv: TextureHandle,
    openai: TextureHandle,
    aliexpress: TextureHandle,
    steam: TextureHandle,
    reddit: TextureHandle,
    spotify: TextureHandle,
    paypal: TextureHandle,
    zoom: TextureHandle,
    whatsapp: TextureHandle,
    netflix: TextureHandle,
    telegram: TextureHandle,
    zed: TextureHandle,
    gnome: TextureHandle,
}

fn load(ctx: &Context, name: &str, svg: &str) -> TextureHandle {
    let image: ColorImage = svg_tex::rasterize_svg(svg, LOGO_PX);
    ctx.load_texture(name, image, TextureOptions::LINEAR)
}

fn pack(ctx: &Context) -> BrandPack {
    // Bump id when brand set changes so cached textures refresh.
    let id = Id::new("ailiakmail_brand_logos_v2");
    if let Some(p) = ctx.data(|d| d.get_temp::<BrandPack>(id)) {
        return p;
    }
    let p = BrandPack {
        google: load(ctx, "brand-google", include_str!("../assets/brands/google.svg")),
        instagram: load(
            ctx,
            "brand-instagram",
            include_str!("../assets/brands/instagram.svg"),
        ),
        linkedin: load(
            ctx,
            "brand-linkedin",
            include_str!("../assets/brands/linkedin.svg"),
        ),
        facebook: load(
            ctx,
            "brand-facebook",
            include_str!("../assets/brands/facebook.svg"),
        ),
        meta: load(ctx, "brand-meta", include_str!("../assets/brands/meta.svg")),
        x: load(ctx, "brand-x", include_str!("../assets/brands/x.svg")),
        github: load(ctx, "brand-github", include_str!("../assets/brands/github.svg")),
        gitlab: load(ctx, "brand-gitlab", include_str!("../assets/brands/gitlab.svg")),
        discord: load(
            ctx,
            "brand-discord",
            include_str!("../assets/brands/discord.svg"),
        ),
        microsoft: load(
            ctx,
            "brand-microsoft",
            include_str!("../assets/brands/microsoft.svg"),
        ),
        apple: load(ctx, "brand-apple", include_str!("../assets/brands/apple.svg")),
        amazon: load(ctx, "brand-amazon", include_str!("../assets/brands/amazon.svg")),
        stripe: load(ctx, "brand-stripe", include_str!("../assets/brands/stripe.svg")),
        notion: load(ctx, "brand-notion", include_str!("../assets/brands/notion.svg")),
        slack: load(ctx, "brand-slack", include_str!("../assets/brands/slack.svg")),
        dropbox: load(
            ctx,
            "brand-dropbox",
            include_str!("../assets/brands/dropbox.svg"),
        ),
        figma: load(ctx, "brand-figma", include_str!("../assets/brands/figma.svg")),
        svgator: load(
            ctx,
            "brand-svgator",
            include_str!("../assets/brands/svgator.svg"),
        ),
        cursor: load(ctx, "brand-cursor", include_str!("../assets/brands/cursor.svg")),
        warp: load(ctx, "brand-warp", include_str!("../assets/brands/warp.svg")),
        abv: load(ctx, "brand-abv", include_str!("../assets/brands/abv.svg")),
        openai: load(ctx, "brand-openai", include_str!("../assets/brands/openai.svg")),
        aliexpress: load(
            ctx,
            "brand-aliexpress",
            include_str!("../assets/brands/aliexpress.svg"),
        ),
        steam: load(ctx, "brand-steam", include_str!("../assets/brands/steam.svg")),
        reddit: load(ctx, "brand-reddit", include_str!("../assets/brands/reddit.svg")),
        spotify: load(
            ctx,
            "brand-spotify",
            include_str!("../assets/brands/spotify.svg"),
        ),
        paypal: load(ctx, "brand-paypal", include_str!("../assets/brands/paypal.svg")),
        zoom: load(ctx, "brand-zoom", include_str!("../assets/brands/zoom.svg")),
        whatsapp: load(
            ctx,
            "brand-whatsapp",
            include_str!("../assets/brands/whatsapp.svg"),
        ),
        netflix: load(
            ctx,
            "brand-netflix",
            include_str!("../assets/brands/netflix.svg"),
        ),
        telegram: load(
            ctx,
            "brand-telegram",
            include_str!("../assets/brands/telegram.svg"),
        ),
        zed: load(ctx, "brand-zed", include_str!("../assets/brands/zed.svg")),
        gnome: load(ctx, "brand-gnome", include_str!("../assets/brands/gnome.svg")),
    };
    ctx.data_mut(|d| d.insert_temp(id, p.clone()));
    p
}

fn tex_for<'a>(pack: &'a BrandPack, brand_id: &str) -> Option<&'a TextureHandle> {
    Some(match brand_id {
        "google" => &pack.google,
        "instagram" => &pack.instagram,
        "linkedin" => &pack.linkedin,
        "facebook" => &pack.facebook,
        "meta" => &pack.meta,
        "x" => &pack.x,
        "github" => &pack.github,
        "gitlab" => &pack.gitlab,
        "discord" => &pack.discord,
        "microsoft" => &pack.microsoft,
        "apple" => &pack.apple,
        "amazon" => &pack.amazon,
        "stripe" => &pack.stripe,
        "notion" => &pack.notion,
        "slack" => &pack.slack,
        "dropbox" => &pack.dropbox,
        "figma" => &pack.figma,
        "svgator" => &pack.svgator,
        "cursor" => &pack.cursor,
        "warp" => &pack.warp,
        "abv" => &pack.abv,
        "openai" => &pack.openai,
        "aliexpress" => &pack.aliexpress,
        "steam" => &pack.steam,
        "reddit" => &pack.reddit,
        "spotify" => &pack.spotify,
        "paypal" => &pack.paypal,
        "zoom" => &pack.zoom,
        "whatsapp" => &pack.whatsapp,
        "netflix" => &pack.netflix,
        "telegram" => &pack.telegram,
        "zed" => &pack.zed,
        "gnome" => &pack.gnome,
        _ => return None,
    })
}

pub fn texture(ctx: &Context, brand_id: &str) -> Option<TextureHandle> {
    tex_for(&pack(ctx), brand_id).cloned()
}
