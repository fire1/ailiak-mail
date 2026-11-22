//! Rasterize SVG → egui [`ColorImage`] via resvg (shared by brand + icons).

use egui::ColorImage;

/// Full-color SVG (logo): straight RGBA from tiny-skia premultiplied output.
pub fn rasterize_svg(svg: &str, target_w: u32) -> ColorImage {
    let (w, h, premul) = render_premul(svg, target_w);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for i in 0..w * h {
        let o = i * 4;
        let r = premul[o];
        let g = premul[o + 1];
        let b = premul[o + 2];
        let a = premul[o + 3];
        if a == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else if a == 255 {
            rgba.extend_from_slice(&[r, g, b, a]);
        } else {
            let af = a as f32;
            rgba.push(((r as f32) * 255.0 / af).round().clamp(0.0, 255.0) as u8);
            rgba.push(((g as f32) * 255.0 / af).round().clamp(0.0, 255.0) as u8);
            rgba.push(((b as f32) * 255.0 / af).round().clamp(0.0, 255.0) as u8);
            rgba.push(a);
        }
    }
    ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}

fn render_premul(svg: &str, target_w: u32) -> (usize, usize, Vec<u8>) {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg, &opt).expect("svg parse");
    let scale = target_w as f32 / tree.size().width().max(1.0);
    let target_h = (tree.size().height() * scale).ceil().max(1.0) as u32;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(target_w, target_h).expect("svg pixmap");
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    (
        pixmap.width() as usize,
        pixmap.height() as usize,
        pixmap.data().to_vec(),
    )
}
