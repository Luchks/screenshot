use minifb::{Key, Window, WindowOptions, Scale};
use std::process::{Command, Stdio};
use std::io::Write;
use image::{GenericImageView, RgbaImage};

fn main() {
    // 1. Captura de pantalla inicial
    Command::new("grim").arg("/tmp/screenshot.png").output().unwrap();
    let img = image::open("/tmp/screenshot.png").expect("Error al abrir captura");
    let (width, height) = img.dimensions();
    let (w, h) = (width as usize, height as usize);

    let buffer: Vec<u32> = img.to_rgba8().pixels().map(|p| {
        ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32)
    }).collect();

    let mut options = WindowOptions::default();
    options.borderless = true; 
    options.title = false;      
    options.scale = Scale::X1;

    let mut window = Window::new("VimShot", w, h, options).expect("Error al abrir");
    window.set_target_fps(60);

    let (mut x, mut y) = (w / 2, h / 2);
    let (mut start_x, mut start_y) = (x, y);
    let mut is_selecting_crop = false; 

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let step = if window.is_key_down(Key::LeftShift) { 40 } else { 10 };

        if window.is_key_down(Key::H) { x = x.saturating_sub(step); }
        if window.is_key_down(Key::L) { x = (x + step).min(w - 1); }
        if window.is_key_down(Key::K) { y = y.saturating_sub(step); }
        if window.is_key_down(Key::J) { y = (y + step).min(h - 1); }

        let (auto_x0, auto_y0, auto_x1, auto_y1) = detect_block(&buffer, w, h, x, y);

        if window.is_key_pressed(Key::V, minifb::KeyRepeat::No) { 
            start_x = auto_x0; start_y = auto_y0;
            x = auto_x1; y = auto_y1;
            is_selecting_crop = true; 
        }

        let mut view = buffer.clone();
        if !is_selecting_crop {
            draw_rect_border(&mut view, w, h, auto_x0, auto_y0, auto_x1, auto_y1, 0x00AAFF);
        } else {
            draw_rect_border(&mut view, w, h, start_x, start_y, x, y, 0xFFFFFF);
        }

        // Render cursor
        for i in -25..=25 {
            if x as i32 + i >= 0 && x as i32 + i < w as i32 { view[y * w + (x as i32 + i) as usize] = 0xFFFFFF; }
            if y as i32 + i >= 0 && y as i32 + i < h as i32 { view[(y as i32 + i) as usize * w + x] = 0xFFFFFF; }
        }

        if window.is_key_down(Key::Enter) {
            // Si no hay selección manual, usamos la detección automática azul
            let final_area = if is_selecting_crop { (start_x, start_y, x, y) } else { (auto_x0, auto_y0, auto_x1, auto_y1) };
            save_and_copy_sturdy(&buffer, w, final_area);
            std::process::exit(0);
        }
        window.update_with_buffer(&view, w, h).unwrap();
    }
}

fn detect_block(buf: &[u32], w: usize, h: usize, cx: usize, cy: usize) -> (usize, usize, usize, usize) {
    let target = buf[cy * w + cx];
    let is_same = |x: usize, y: usize| {
        let p = buf[y * w + x];
        let diff = ((p >> 16) & 0xFF).abs_diff((target >> 16) & 0xFF) +
                   ((p >> 8) & 0xFF).abs_diff((target >> 8) & 0xFF) +
                   (p & 0xFF).abs_diff(target & 0xFF);
        diff < 15 
    };
    let (mut x0, mut x1, mut y0, mut y1) = (cx, cx, cy, cy);
    while x0 > 0 && is_same(x0 - 1, cy) { x0 -= 1; }
    while x1 < w - 1 && is_same(x1 + 1, cy) { x1 += 1; }
    while y0 > 0 && is_same(cx, y0 - 1) { y0 -= 1; }
    while y1 < h - 1 && is_same(cx, y1 + 1) { y1 += 1; }
    (x0, y0, x1, y1)
}

fn draw_rect_border(buf: &mut [u32], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize, color: u32) {
    let (xmin, xmax) = (x0.min(x1), x0.max(x1));
    let (ymin, ymax) = (y0.min(y1), y0.max(y1));
    for i in xmin..=xmax {
        if i < w {
            if ymin < h { buf[ymin * w + i] = color; }
            if ymax < h { buf[ymax * w + i] = color; }
        }
    }
    for i in ymin..=ymax {
        if i < h {
            if xmin < w { buf[i * w + xmin] = color; }
            if xmax < w { buf[i * w + xmax] = color; }
        }
    }
}

fn save_and_copy_sturdy(buffer: &[u32], w: usize, area: (usize, usize, usize, usize)) {
    let (x0, y0, x1, y1) = area;
    let (xmin, xmax) = (x0.min(x1), x0.max(x1));
    let (ymin, ymax) = (y0.min(y1), y0.max(y1));
    let (cw, ch) = (xmax - xmin, ymax - ymin);
    
    if cw == 0 || ch == 0 { return; }
    
    let mut out = RgbaImage::new(cw as u32, ch as u32);
    for py in 0..ch {
        for px in 0..cw {
            let p = buffer[(ymin + py) * w + (xmin + px)];
            out.put_pixel(px as u32, py as u32, image::Rgba([
                ((p >> 16) & 0xFF) as u8, 
                ((p >> 8) & 0xFF) as u8, 
                (p & 0xFF) as u8, 
                255
            ]));
        }
    }

    // Codificar a PNG en memoria
    let mut png_data: Vec<u8> = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_data);
    out.write_to(&mut cursor, image::ImageFormat::Png).ok();

    // Copiar al portapapeles inyectando los datos directamente
    let mut child = Command::new("wl-copy")
        .arg("--type")
        .arg("image/png")
        .stdin(Stdio::piped())
        .spawn()
        .expect("Error al copiar");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&png_data).ok();
        stdin.flush().ok();
    }
    
    // Dejamos que wl-copy respire antes de matar el proceso principal
    std::thread::sleep(std::time::Duration::from_millis(200));
}
