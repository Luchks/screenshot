use minifb::{Key, Window, WindowOptions};
use std::process::Command;
use image::{GenericImageView, RgbaImage};

fn main() {
    // 1. Captura de pantalla silenciosa
    Command::new("grim").arg("/tmp/screenshot.png").output().unwrap();
    
    let img = image::open("/tmp/screenshot.png").expect("Error al abrir captura");
    let (width, height) = img.dimensions();
    let (w, h) = (width as usize, height as usize);

    let mut buffer: Vec<u32> = img.to_rgba8().pixels().map(|p| {
        ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32)
    }).collect();

    let mut window = Window::new(
        "VimShot - ENTER: Copiar | v: Inicio | a: Flecha | c: Círculo | r: Rectángulo",
        w, h, WindowOptions::default()
    ).expect("No se pudo abrir la ventana");
    
    window.set_target_fps(60);

    let (mut x, mut y) = (w / 2, h / 2);
    let (mut start_x, mut start_y) = (x, y);

    while window.is_open() && !window.is_key_down(Key::Escape) {

        let step = if window.is_key_down(Key::LeftShift) { 25 } else { 5 };
        if window.is_key_down(Key::H) && x > step { x -= step; }
        if window.is_key_down(Key::L) && x < w - step { x += step; }
        if window.is_key_down(Key::K) && y > step { y -= step; }
        if window.is_key_down(Key::J) && y < h - step { y += step; }

        // Comandos Vim
        if window.is_key_down(Key::V) { start_x = x; start_y = y; }
        if window.is_key_down(Key::A) { draw_line(&mut buffer, w, start_x, start_y, x, y, 0xFF0000); }
        if window.is_key_down(Key::C) {
            let r = (((x as f32 - start_x as f32).powi(2) + (y as f32 - start_y as f32).powi(2)).sqrt()) as usize;
            draw_circle(&mut buffer, w, h, start_x, start_y, r, 0x0000FF);
        }
        
        // --- NUEVA FUNCIONALIDAD: RECTÁNGULO ---
        if window.is_key_down(Key::R) {
            draw_rect(&mut buffer, w, h, start_x, start_y, x, y, 0xFFFF00); // Amarillo
        }

        if window.is_key_down(Key::Enter) {
            let temp_path = "/tmp/vimshot_done.png";
            let mut out_img = RgbaImage::new(width, height);
            for (idx, pixel_u32) in buffer.iter().enumerate() {
                let px = (idx % w) as u32;
                let py = (idx / w) as u32;
                let r = ((pixel_u32 >> 16) & 0xFF) as u8;
                let g = ((pixel_u32 >> 8) & 0xFF) as u8;
                let b = (pixel_u32 & 0xFF) as u8;
                out_img.put_pixel(px, py, image::Rgba([r, g, b, 255]));
            }
            out_img.save(temp_path).unwrap();

            Command::new("sh")
                .arg("-c")
                .arg(format!("wl-copy --type image/png < {} && rm {}", temp_path, temp_path))
                .spawn()
                .expect("Fallo al copiar");

            std::process::exit(0);
        }

        if window.is_key_down(Key::D) {
            buffer = img.to_rgba8().pixels().map(|p| {
                ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32)
            }).collect();
        }

        let mut view = buffer.clone();
        for i in 0..20 {
            let idx_h = y * w + x.saturating_add(i).saturating_sub(10);
            let idx_v = x.saturating_add(y.saturating_add(i).saturating_sub(10) * w);
            if idx_h < view.len() { view[idx_h] = 0xFFFFFF; }
            if idx_v < view.len() { view[idx_v] = 0xFFFFFF; }
        }

        window.update_with_buffer(&view, w, h).unwrap();
    }
}

// --- NUEVA FUNCIÓN PARA DIBUJAR RECTÁNGULOS ---
fn draw_rect(buf: &mut Vec<u32>, w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize, color: u32) {
    let x_min = x0.min(x1);
    let x_max = x0.max(x1);
    let y_min = y0.min(y1);
    let y_max = y0.max(y1);

    for x in x_min..=x_max {
        // Línea superior e inferior
        if y_min * w + x < buf.len() { buf[y_min * w + x] = color; }
        if y_max * w + x < buf.len() { buf[y_max * w + x] = color; }
    }
    for y in y_min..=y_max {
        // Línea izquierda y derecha
        if y * w + x_min < buf.len() { buf[y * w + x_min] = color; }
        if y * w + x_max < buf.len() { buf[y * w + x_max] = color; }
    }
}

fn draw_line(buf: &mut Vec<u32>, w: usize, x0: usize, y0: usize, x1: usize, y1: usize, color: u32) {
    let mut x0 = x0 as i32; let mut y0 = y0 as i32;
    let x1 = x1 as i32; let y1 = y1 as i32;
    let dx = (x1 - x0).abs(); let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        let idx = (y0 as usize) * w + (x0 as usize);
        if idx < buf.len() { buf[idx] = color; }
        if x0 == x1 && y0 == y1 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; x0 += sx; }
        if e2 <= dx { err += dx; y0 += sy; }
    }
}

fn draw_circle(buf: &mut Vec<u32>, w: usize, h: usize, cx: usize, cy: usize, r: usize, color: u32) {
    let mut x = r as i32; let mut y = 0i32;
    let mut err = 0i32;
    while x >= y {
        let pts = [(cx as i32 + x, cy as i32 + y), (cx as i32 - x, cy as i32 + y), (cx as i32 + x, cy as i32 - y), (cx as i32 - x, cy as i32 - y), (cx as i32 + y, cy as i32 + x), (cx as i32 - y, cy as i32 + x), (cx as i32 + y, cy as i32 - x), (cx as i32 - y, cy as i32 - x)];
        for (px, py) in pts {
            if px >= 0 && px < w as i32 && py >= 0 && py < h as i32 {
                buf[(py as usize) * w + (px as usize)] = color;
            }
        }
        y += 1;
        if err <= 0 { err += 2 * y + 1; }
        else { x -= 1; err += 2 * (y - x) + 1; }
    }
}
