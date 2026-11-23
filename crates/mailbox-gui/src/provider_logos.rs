//! Provider brand marks — one size for every card (SVG → 64×64 texture).

use egui::{ColorImage, Context, Id, TextureHandle, TextureOptions};
use mailbox_core::providers::MailProvider;

use crate::svg_tex;

const LOGO_PX: u32 = 64;

/// On-screen size for every provider mark (cards + credentials header).
pub const DISPLAY_SIZE: f32 = 28.0;

#[derive(Clone)]
struct LogoPack {
    gmail: TextureHandle,
    yahoo: TextureHandle,
    office365: TextureHandle,
    outlook: TextureHandle,
    icloud: TextureHandle,
    yandex: TextureHandle,
    gmx: TextureHandle,
    imap: TextureHandle,
    pop3: TextureHandle,
}

fn load(ctx: &Context, name: &str, svg: &str) -> TextureHandle {
    let image: ColorImage = svg_tex::rasterize_svg(svg, LOGO_PX);
    debug_assert_eq!(image.width(), LOGO_PX as usize);
    debug_assert_eq!(image.height(), LOGO_PX as usize);
    ctx.load_texture(name, image, TextureOptions::LINEAR)
}

fn pack(ctx: &Context) -> LogoPack {
    let id = Id::new("ailiakmail_provider_logos_v1");
    if let Some(p) = ctx.data(|d| d.get_temp::<LogoPack>(id)) {
        return p;
    }
    let p = LogoPack {
        gmail: load(ctx, "prov-gmail", include_str!("../assets/providers/gmail.svg")),
        yahoo: load(ctx, "prov-yahoo", include_str!("../assets/providers/yahoo.svg")),
        office365: load(
            ctx,
            "prov-office365",
            include_str!("../assets/providers/office365.svg"),
        ),
        outlook: load(
            ctx,
            "prov-outlook",
            include_str!("../assets/providers/outlook.svg"),
        ),
        icloud: load(ctx, "prov-icloud", include_str!("../assets/providers/icloud.svg")),
        yandex: load(ctx, "prov-yandex", include_str!("../assets/providers/yandex.svg")),
        gmx: load(ctx, "prov-gmx", include_str!("../assets/providers/gmx.svg")),
        imap: load(ctx, "prov-imap", include_str!("../assets/providers/imap.svg")),
        pop3: load(ctx, "prov-pop3", include_str!("../assets/providers/pop3.svg")),
    };
    ctx.data_mut(|d| d.insert_temp(id, p.clone()));
    p
}

fn tex_for<'a>(pack: &'a LogoPack, provider: MailProvider) -> &'a TextureHandle {
    match provider {
        MailProvider::Gmail => &pack.gmail,
        MailProvider::Yahoo => &pack.yahoo,
        MailProvider::Office365 => &pack.office365,
        MailProvider::Outlook => &pack.outlook,
        MailProvider::Icloud => &pack.icloud,
        MailProvider::Yandex => &pack.yandex,
        MailProvider::Gmx => &pack.gmx,
        MailProvider::Abv => &pack.pop3,
        MailProvider::Imap => &pack.imap,
        MailProvider::Pop3 => &pack.pop3,
    }
}

pub fn texture(ctx: &Context, provider: MailProvider) -> TextureHandle {
    tex_for(&pack(ctx), provider).clone()
}
