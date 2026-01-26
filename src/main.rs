use minifb::{Key, Window, WindowOptions, Scale};
use std::process::Command;
use image::{GenericImageView, RgbaImage};

fn main() {
    Command::new("grim").arg("/tmp/screenshot.png").output().unwrap();
    let img = image::open("/tmp/screenshot.png").expect("Error al abrir captura");
    let (width, height) = img.dimensions();
    let (w, h) = (width as usize, height as usize);

    let mut buffer: Vec<u32> = img.to_rgba8().pixels().map(|p| {
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
    let mut last_r_state = false;
    let mut last_f_state = false;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        
        // --- DETECCIÓN DE VELOCIDAD MEJORADA ---
        // Usamos solo el estado físico de la tecla para ignorar Bloq Mayús
        let is_turbo = window.is_key_down(Key::LeftShift) || window.is_key_down(Key::RightShift);
        let is_precise = window.is_key_down(Key::LeftAlt) || window.is_key_down(Key::LeftCtrl);

        let step = if is_turbo { 25 } else if is_precise { 1 } else { 5 };

        // Movimiento HJKL
        if window.is_key_down(Key::H) { x = x.saturating_sub(step); }
        if window.is_key_down(Key::L) { x = (x + step).min(w - 1); }
        if window.is_key_down(Key::K) { y = y.saturating_sub(step); }
        if window.is_key_down(Key::J) { y = (y + step).min(h - 1); }

        if window.is_key_down(Key::V) { start_x = x; start_y = y; }
        if window.is_key_down(Key::A) { draw_line(&mut buffer, w, start_x, start_y, x, y, 0xFF0000); }
        
        if window.is_key_down(Key::C) {
            let r = (((x as f32 - start_x as f32).powi(2) + (y as f32 - start_y as f32).powi(2)).sqrt()) as usize;
            draw_circle(&mut buffer, w, h, start_x, start_y, r, 0x0000FF);
        }

        let current_r_state = window.is_key_down(Key::R);
        if !current_r_state && last_r_state {
            draw_filled_rect(&mut buffer, w, h, start_x, start_y, x, y, 0xFFFF00, 0.35);
        }
        last_r_state = current_r_state;

        let current_f_state = window.is_key_down(Key::F);
        if !current_f_state && last_f_state {
            draw_arrow(&mut buffer, w, start_x, start_y, x, y, 0xFF00FF);
        }
        last_f_state = current_f_state;

        if window.is_key_down(Key::D) {
            buffer = img.to_rgba8().pixels().map(|p| { ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32) }).collect();
        }

        let mut view = buffer.clone();
        if current_r_state { draw_filled_rect(&mut view, w, h, start_x, start_y, x, y, 0xFFFF00, 0.35); }
        if current_f_state { draw_arrow(&mut view, w, start_x, start_y, x, y, 0xFF00FF); }

        render_cursor(&mut view, w, h, x, y);

        if window.is_key_down(Key::Enter) {
            save_and_copy(&buffer, w, width, height);
            std::process::exit(0);
        }
        window.update_with_buffer(&view, w, h).unwrap();
    }
}

// --- FUNCIONES DE SOPORTE (Idénticas a las anteriores) ---
fn render_cursor(view: &mut Vec<u32>, w: usize, h: usize, x: usize, y: usize) {
    let c_size = 12;
    for i in -c_size..=c_size {
        let pts = [(x as i32 + i, y as i32), (x as i32, y as i32 + i)];
        for &(px, py) in &pts {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let nx = px + dx; let ny = py + dy;
                    if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                        view[(ny as usize) * w + (nx as usize)] = 0x000000;
                    }
                }
            }
        }
    }
    for i in -c_size..=c_size {
        let pts = [(x as i32 + i, y as i32), (x as i32, y as i32 + i)];
        for &(px, py) in &pts {
            if px >= 0 && px < w as i32 && py >= 0 && py < h as i32 {
                view[(py as usize) * w + (px as usize)] = 0xFFFFFF;
            }
        }
    }
}

fn save_and_copy(buffer: &Vec<u32>, w: usize, width: u32, height: u32) {
    let temp_path = "/tmp/vimshot_done.png";
    let mut out_img = RgbaImage::new(width, height);
    for (idx, pixel_u32) in buffer.iter().enumerate() {
        let px = (idx % w) as u32; let py = (idx / w) as u32;
        let r = ((pixel_u32 >> 16) & 0xFF) as u8;
        let g = ((pixel_u32 >> 8) & 0xFF) as u8;
        let b = (pixel_u32 & 0xFF) as u8;
        out_img.put_pixel(px, py, image::Rgba([r, g, b, 255]));
    }
    out_img.save(temp_path).unwrap();
    Command::new("sh").arg("-c").arg(format!("wl-copy --type image/png < {} && rm {}", temp_path, temp_path)).spawn().unwrap();
}

fn draw_arrow(buf: &mut Vec<u32>, w: usize, x0: usize, y0: usize, x1: usize, y1: usize, color: u32) {
    draw_line(buf, w, x0, y0, x1, y1, color);
    let dx = x1 as f32 - x0 as f32;
    let dy = y1 as f32 - y0 as f32;
    let angle = dy.atan2(dx);
    let arrow_size = 20.0;
    let wing_angle = 0.5;
    let x2 = x1 as f32 - arrow_size * (angle - wing_angle).cos();
    let y2 = y1 as f32 - arrow_size * (angle - wing_angle).sin();
    draw_line(buf, w, x1, y1, x2 as usize, y2 as usize, color);
    let x3 = x1 as f32 - arrow_size * (angle + wing_angle).cos();
    let y3 = y1 as f32 - arrow_size * (angle + wing_angle).sin();
    draw_line(buf, w, x1, y1, x3 as usize, y3 as usize, color);
}

fn draw_filled_rect(buf: &mut Vec<u32>, w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize, color: u32, alpha: f32) {
    let x_min = x0.min(x1); let x_max = x0.max(x1);
    let y_min = y0.min(y1); let y_max = y0.max(y1);
    let r_h = ((color >> 16) & 0xFF) as f32;
    let g_h = ((color >> 8) & 0xFF) as f32;
    let b_h = (color & 0xFF) as f32;
    for py in y_min..=y_max {
        for px in x_min..=x_max {
            if py < h && px < w {
                let idx = py * w + px;
                let old = buf[idx];
                let r = (r_h * alpha + ((old >> 16) & 0xFF) as f32 * (1.0 - alpha)) as u32;
                let g = (g_h * alpha + ((old >> 8) & 0xFF) as f32 * (1.0 - alpha)) as u32;
                let b = (b_h * alpha + (old & 0xFF) as f32 * (1.0 - alpha)) as u32;
                buf[idx] = (r << 16) | (g << 8) | b;
            }
        }
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
        if x0 >= 0 && x0 < w as i32 && y0 >= 0 && y0 < (buf.len()/w) as i32 {
            buf[(y0 as usize) * w + (x0 as usize)] = color;
        }
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
