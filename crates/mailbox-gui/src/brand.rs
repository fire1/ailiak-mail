//! Ailiak mark (from `../Ailiak.com` favicon / logo assets).

use egui::{ColorImage, Context, TextureHandle, TextureOptions};

pub fn load_ailiak_mark(ctx: &Context) -> TextureHandle {
    let bytes = include_bytes!("../assets/img/ailiak-mark.png");
    let img = image::load_from_memory(bytes)
        .expect("ailiak-mark.png")
        .to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    ctx.load_texture("ailiak-mark", color, TextureOptions::LINEAR)
}
