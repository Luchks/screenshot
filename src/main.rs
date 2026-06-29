use minifb::{Key, Window, WindowOptions, Scale};
use std::process::{Command, Stdio};
use std::io::Write;
use std::time::{Instant, Duration};
use image::{GenericImageView, RgbaImage};

// ─── Módulos añadidos (Etapa 3) ──────────────────────────────────────────────
mod features;
mod classify;
mod hierarchy;
mod interaction;                                                    // Etapa 4

use hierarchy::{build_ui_semantic_layer, print_ui_tree};
use interaction::{InteractionEngine, SelectionTarget};             // Etapa 4

// ─── Tipos base ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    xmin: usize,
    ymin: usize,
    xmax: usize,
    ymax: usize,
}

impl Rect {
    fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.xmin && x <= self.xmax && y >= self.ymin && y <= self.ymax
    }

    fn expand(&self, margin: usize, max_w: usize, max_h: usize) -> Rect {
        Rect {
            xmin: self.xmin.saturating_sub(margin),
            ymin: self.ymin.saturating_sub(margin),
            xmax: (self.xmax + margin).min(max_w.saturating_sub(1)),
            ymax: (self.ymax + margin).min(max_h.saturating_sub(1)),
        }
    }

    fn clamp_to_screen(&self, max_w: usize, max_h: usize) -> Rect {
        Rect {
            xmin: self.xmin.min(max_w.saturating_sub(1)),
            ymin: self.ymin.min(max_h.saturating_sub(1)),
            xmax: self.xmax.min(max_w.saturating_sub(1)),
            ymax: self.ymax.min(max_h.saturating_sub(1)),
        }
    }

    fn union_with(&self, other: Rect) -> Rect {
        Rect {
            xmin: self.xmin.min(other.xmin),
            ymin: self.ymin.min(other.ymin),
            xmax: self.xmax.max(other.xmax),
            ymax: self.ymax.max(other.ymax),
        }
    }
}

// ─── Region: abstracción de segmento para pipeline futuro ─────────────────────

#[derive(Clone, Debug)]
struct Region {
    id: usize,
    bbox: Rect,
    area: usize,
    perimeter: usize,
    mean_color: (f32, f32, f32),
    color_variance: f32,
    neighbors: Vec<usize>,
}

// ─── HintMode: modelo de datos ────────────────────────────────────────────────

struct CandidateId(usize);

struct CandidateMetrics {
    area: f32,
    distance: f32,
    aspect_ratio: f32,
}

struct CandidateWeights {
    area: f32,
    distance: f32,
    aspect_ratio: f32,
}

impl Default for CandidateWeights {
    fn default() -> Self {
        CandidateWeights {
            area:         0.4,
            distance:     0.4,
            aspect_ratio: 0.2,
        }
    }
}

struct Candidate {
    id:          CandidateId,
    center_x:    usize,
    center_y:    usize,
    metrics:     CandidateMetrics,
    total_score: f32,
}

struct Hint {
    label:           String,
    candidate_index: usize,
}

// ─── Máquina de estados ───────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum SelectionMode {
    AutoSnap,
    ManualResize,
    ManualVisual,
    HintMode,
}

#[derive(Clone, Copy, Debug)]
struct Selection {
    rect: Rect,
    mode: SelectionMode,
    source_label: Option<u32>,
}

#[derive(Clone, Copy, PartialEq)]
struct FrameState {
    cursor_x: usize,
    cursor_y: usize,
    rect: Rect,
    mode: SelectionMode,
}

// ─── Colores por modo ─────────────────────────────────────────────────────────

fn border_color(mode: SelectionMode) -> u32 {
    match mode {
        SelectionMode::AutoSnap     => 0x00AAFF,
        SelectionMode::ManualResize => 0xFF8800,
        SelectionMode::ManualVisual => 0x00FF88,
        SelectionMode::HintMode     => 0x00AAFF,
    }
}

// ─── Estructuras e Implementación de Regiones de parche.md ────────────────────

struct Dsu {
    parent: Vec<usize>,
    size:   Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Dsu {
            parent: (0..n).collect(),
            size:   vec![1; n],
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]]; // path halving
            i = self.parent[i];
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) -> bool {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb { return false; }
        if self.size[ra] >= self.size[rb] {
            self.parent[rb] = ra;
            self.size[ra] += self.size[rb];
        } else {
            self.parent[ra] = rb;
            self.size[rb] += self.size[ra];
        }
        true
    }
}

#[derive(Clone, Copy)]
struct MergeEdge {
    cost: f32,
    a:    usize,
    b:    usize,
}

const COLOR_WEIGHT:      f32 = 0.55;
const SPATIAL_WEIGHT:    f32 = 0.30;
const SIZE_RATIO_WEIGHT: f32 = 0.15;
const BASE_K:            f32 = 300.0;
const SIZE_SCALE:        f32 = 0.5;

fn merge_cost(a: &Region, b: &Region, w: usize, h: usize) -> f32 {
    let dr = a.mean_color.0 - b.mean_color.0;
    let dg = a.mean_color.1 - b.mean_color.1;
    let db = a.mean_color.2 - b.mean_color.2;
    let color_dist = (dr * dr + dg * dg + db * db).sqrt() / 441.67;

    let gap_x = if a.bbox.xmax < b.bbox.xmin {
        (b.bbox.xmin - a.bbox.xmax) as f32
    } else if b.bbox.xmax < a.bbox.xmin {
        (a.bbox.xmin - b.bbox.xmax) as f32
    } else {
        0.0
    };
    let gap_y = if a.bbox.ymax < b.bbox.ymin {
        (b.bbox.ymin - a.bbox.ymax) as f32
    } else if b.bbox.xmax < a.bbox.xmin {
        (a.bbox.xmin - b.bbox.xmax) as f32
    } else {
        0.0
    };
    let diag = ((w * w + h * h) as f32).sqrt();
    let spatial = (gap_x * gap_x + gap_y * gap_y).sqrt() / diag;

    let sa = a.area.max(1) as f32;
    let sb = b.area.max(1) as f32;
    let ratio = if sa > sb { sb / sa } else { sa / sb };
    let size_penalty = 4.0 * ratio * (1.0 - ratio);

    COLOR_WEIGHT * color_dist + SPATIAL_WEIGHT * spatial + SIZE_RATIO_WEIGHT * size_penalty
}

fn mint(area: usize) -> f32 {
    BASE_K / (area.max(1) as f32).powf(SIZE_SCALE)
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    Command::new("grim").arg("/tmp/screenshot.png").output().unwrap();
    let img = image::open("/tmp/screenshot.png").expect("Error al abrir captura");
    let (width, height) = img.dimensions();
    let (w, h) = (width as usize, height as usize);

    let clean_buffer: Vec<u32> = img.to_rgba8().pixels().map(|p| {
        ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32)
    }).collect();

    let mut render_buffer = clean_buffer.clone();

    println!("Analizando estructura de la interfaz (Pipeline)...");
    let (mut label_map, mut regions) = fh_segment(&clean_buffer, w, h);
    regions = merge_regions(
        &mut label_map,
        regions,
        &clean_buffer,
        w,
        h,
    );
    let blocks = build_region_tree(&regions);
    println!("Se detectaron {} bloques visuales independientes.", blocks.len());

    /* ▶ INICIO BLOQUE DE INTEGRACIÓN (Etapa 3) ----------------------------- */
    println!("Construyendo capa semántica UI...");
    let (ui_elements, ui_tree) = build_ui_semantic_layer(&regions, w, h);

    // Debug en consola (quitar en producción o mover a flag --debug)
    print_ui_tree(&ui_tree);

    println!(
        "Etapa 3 completa: {} elementos UI clasificados ({} raíces en el árbol).",
        ui_elements.len(),
        ui_tree.roots.len(),
    );
    /* ▶ FIN BLOQUE DE INTEGRACIÓN --------------------------------------------- */

    // ─── Etapa 4: Motor de Interacción Semántica ─────────────────────────────
    let engine = InteractionEngine::new();
    // ─────────────────────────────────────────────────────────────────────────

    let snap_rect = |cx: usize, cy: usize| -> (Rect, Option<u32>) {
        let lbl = label_map[cy * w + cx];
        if lbl > 0 && (lbl as usize) <= blocks.len() {
            (blocks[(lbl - 1) as usize], Some(lbl))
        } else {
            (Rect { xmin: cx, ymin: cy, xmax: cx, ymax: cy }, None)
        }
    };

    let mut options = WindowOptions::default();
    options.borderless = true;
    options.title = false;
    options.scale = Scale::X1;

    let mut window = Window::new("VimShot", w, h, options)
        .expect("Error al abrir la ventana");
    window.set_target_fps(60);

    let (mut cx, mut cy) = (w / 2, h / 2);
    let (init_rect, init_label) = engine
        .compute_smart_snap(&ui_tree, cx, cy)
        .and_then(|t| engine.resolve_rect(&ui_tree, &t))
        .map(|r| (r, None::<u32>))
        .unwrap_or_else(|| snap_rect(cx, cy));

    let mut sel = Selection {
        rect: init_rect,
        mode: SelectionMode::AutoSnap,
        source_label: init_label,
    };

    const SNAP_HYSTERESIS: usize = 8;
    let mut visual_origin: Option<(usize, usize)> = None;
    let mut last_input_time = Instant::now();
    let mut v_key_pressed = false;
    let mut f_key_pressed = false;
    let mut hint_key_pressed: Option<Key> = None;

    let mut dirty: Option<Rect> = None;
    let mut prev_frame: Option<FrameState> = None;

    let mut active_candidates: Vec<Candidate> = Vec::new();
    let mut active_hints: Vec<Hint> = Vec::new();
    let mut hint_prefix: Option<char> = None;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let now = Instant::now();
        let shift = window.is_key_down(Key::LeftShift);
        let input_delay = if shift { Duration::from_millis(4) } else { Duration::from_millis(16) };

        if !window.is_key_down(Key::V) { v_key_pressed = false; }
        if !window.is_key_down(Key::F) { f_key_pressed = false; }
        if let Some(hk) = hint_key_pressed {
            if !window.is_key_down(hk) { hint_key_pressed = None; }
        }

        if now.duration_since(last_input_time) >= input_delay {
            let move_step = if shift { 25 } else { 3 };
            let resize_step = if shift { 20 } else { 5 };
            let mut acted = false;

            match sel.mode {
                SelectionMode::AutoSnap => {
                    if window.is_key_down(Key::H) { cx = cx.saturating_sub(move_step); acted = true; }
                    if window.is_key_down(Key::L) { cx = (cx + move_step).min(w - 1);  acted = true; }
                    if window.is_key_down(Key::K) { cy = cy.saturating_sub(move_step); acted = true; }
                    if window.is_key_down(Key::J) { cy = (cy + move_step).min(h - 1);  acted = true; }

                    if acted {
                        if !sel.rect.expand(SNAP_HYSTERESIS, w, h).contains(cx, cy) {
                            // Snap semántico: prioriza importancia sobre distancia pura
                            let semantic = engine
                                .compute_smart_snap(&ui_tree, cx, cy)
                                .and_then(|t| {
                                    // Log de acción contextual (quitar en producción o usar flag --debug)
                                    if let SelectionTarget::Element(id) = &t {
                                        let el = &ui_tree.elements[*id];
                                        let action = engine.infer_action(el.element_type);
                                        eprintln!(
                                            "[snap] id={} type={} action={}",
                                            id,
                                            el.element_type.label(),
                                            action.label()
                                        );
                                    }
                                    engine.resolve_rect(&ui_tree, &t)
                                })
                                .map(|r| (r, None::<u32>));

                            let (r, lbl) = semantic.unwrap_or_else(|| snap_rect(cx, cy));
                            sel.rect = r;
                            sel.source_label = lbl;
                        }
                    }

                    if window.is_key_down(Key::R) { sel.mode = SelectionMode::ManualResize; acted = true; }

                    if window.is_key_down(Key::V) && !v_key_pressed {
                        v_key_pressed = true;
                        visual_origin = Some((cx, cy));
                        sel.mode = SelectionMode::ManualVisual;
                        sel.source_label = None;
                        acted = true;
                    }

                    if window.is_key_down(Key::F) && !f_key_pressed {
                        f_key_pressed = true;
                        let valid = filter_noise(&blocks, w, h);
                        active_candidates = rank_candidates(&valid, &blocks, cx, cy);
                        active_hints = generate_hints(&active_candidates);
                        sel.mode = SelectionMode::HintMode;
                        acted = true;
                    }
                }

                SelectionMode::ManualResize => {
                    if !shift {
                        if window.is_key_down(Key::H) { cx = cx.saturating_sub(move_step); acted = true; }
                        if window.is_key_down(Key::L) { cx = (cx + move_step).min(w - 1);  acted = true; }
                        if window.is_key_down(Key::K) { cy = cy.saturating_sub(move_step); acted = true; }
                        if window.is_key_down(Key::J) { cy = (cy + move_step).min(h - 1);  acted = true; }

                        if acted && !sel.rect.contains(cx, cy) {
                            let (r, lbl) = snap_rect(cx, cy);
                            sel = Selection { rect: r, mode: SelectionMode::AutoSnap, source_label: lbl };
                        }
                    } else {
                        let mut r = sel.rect;
                        if window.is_key_down(Key::H) { r.xmin = r.xmin.saturating_sub(resize_step); acted = true; }
                        if window.is_key_down(Key::L) { r.xmax = (r.xmax + resize_step).min(w - 1);  acted = true; }
                        if window.is_key_down(Key::K) { r.ymin = r.ymin.saturating_sub(resize_step); acted = true; }
                        if window.is_key_down(Key::J) { r.ymax = (r.ymax + resize_step).min(h - 1);  acted = true; }

                        if r.xmin <= r.xmax && r.ymin <= r.ymax {
                            sel.rect = r.clamp_to_screen(w, h);
                        }
                    }

                    if window.is_key_down(Key::Key0) {
                        let (r, lbl) = snap_rect(cx, cy);
                        sel = Selection { rect: r, mode: SelectionMode::AutoSnap, source_label: lbl };
                        acted = true;
                    }
                }

                SelectionMode::ManualVisual => {
                    if window.is_key_down(Key::H) { cx = cx.saturating_sub(move_step); acted = true; }
                    if window.is_key_down(Key::L) { cx = (cx + move_step).min(w - 1);  acted = true; }
                    if window.is_key_down(Key::K) { cy = cy.saturating_sub(move_step); acted = true; }
                    if window.is_key_down(Key::J) { cy = (cy + move_step).min(h - 1);  acted = true; }

                    if acted {
                        if let Some((ox, oy)) = visual_origin {
                            sel.rect = Rect {
                                xmin: ox.min(cx),
                                ymin: oy.min(cy),
                                xmax: ox.max(cx),
                                ymax: oy.max(cy),
                            }.clamp_to_screen(w, h);
                        }
                    }

                    if window.is_key_down(Key::V) && !v_key_pressed {
                        v_key_pressed = true;
                        let (r, lbl) = snap_rect(cx, cy);
                        sel = Selection { rect: r, mode: SelectionMode::AutoSnap, source_label: lbl };
                        visual_origin = None;
                        acted = true;
                    }
                }

                SelectionMode::HintMode => {
                    if window.is_key_down(Key::F) && !f_key_pressed {
                        f_key_pressed = true;
                        active_candidates.clear();
                        active_hints.clear();
                        hint_prefix = None;
                        let (r, lbl) = snap_rect(cx, cy);
                        sel = Selection { rect: r, mode: SelectionMode::AutoSnap, source_label: lbl };
                        acted = true;
                    }

                    if hint_key_pressed.is_none() {
                        for &hk in HINT_KEYS {
                            if window.is_key_down(hk) {
                                hint_key_pressed = Some(hk);
                                if let Some(ch) = key_to_char(hk) {
                                    match hint_prefix {
                                        None => {
                                            hint_prefix = Some(ch);
                                        }
                                        Some(first) => {
                                            let label = format!("{}{}", first, ch);
                                            if let Some(candidate_index) = find_hint_by_label(&active_hints, &label) {
                                                let candidate = &active_candidates[candidate_index];
                                                let rect = &blocks[candidate.id.0];
                                                cx = (rect.xmin + rect.xmax) / 2;
                                                cy = (rect.ymin + rect.ymax) / 2;
                                                let (snapped, lbl) = snap_rect(cx, cy);
                                                sel = Selection {
                                                    rect: snapped,
                                                    mode: SelectionMode::AutoSnap,
                                                    source_label: lbl,
                                                };
                                                active_candidates.clear();
                                                active_hints.clear();
                                            }
                                            hint_prefix = None;
                                        }
                                    }
                                }
                                acted = true;
                                break;
                            }
                        }
                    }
                }
            }

            if acted { last_input_time = now; }
        }

        if window.is_key_down(Key::Enter) {
            let capture_rect = sel.rect.clamp_to_screen(w, h);
            drop(window);
            save_and_copy_sturdy(
                &clean_buffer, w,
                (capture_rect.xmin, capture_rect.ymin, capture_rect.xmax, capture_rect.ymax),
            );
            std::process::exit(0);
        }

        let current_frame = FrameState {
            cursor_x: cx,
            cursor_y: cy,
            rect: sel.rect,
            mode: sel.mode,
        };

        if prev_frame.map_or(true, |pf| pf != current_frame) {
            if let Some(d) = dirty {
                restore_bounding_box(&clean_buffer, &mut render_buffer, w, d);
            }

            if sel.mode == SelectionMode::HintMode {
                let hint_dirty = draw_hint_borders(&mut render_buffer, w, h, &active_candidates, &blocks);
                draw_hints(&mut render_buffer, w, h, &active_hints, &active_candidates);
                draw_cross_cursor(&mut render_buffer, w, h, cx, cy, 0xFFFFFF);

                let cursor_area = Rect {
                    xmin: cx.saturating_sub(30),
                    ymin: cy.saturating_sub(30),
                    xmax: (cx + 30).min(w - 1),
                    ymax: (cy + 30).min(h - 1),
                };

                dirty = Some(match hint_dirty {
                    Some(hd) => hd.union_with(cursor_area),
                    None     => cursor_area,
                });
            } else {
                let color = border_color(sel.mode);
                draw_rect_border(&mut render_buffer, w, h, sel.rect, color);
                draw_cross_cursor(&mut render_buffer, w, h, cx, cy, 0xFFFFFF);

                let cursor_area = Rect {
                    xmin: cx.saturating_sub(30),
                    ymin: cy.saturating_sub(30),
                    xmax: (cx + 30).min(w - 1),
                    ymax: (cy + 30).min(h - 1),
                };
                dirty = Some(sel.rect.union_with(cursor_area));
            }

            window.update_with_buffer(&render_buffer, w, h).unwrap();
            prev_frame = Some(current_frame);
        } else {
            window.update_with_buffer(&render_buffer, w, h).unwrap();
        }
    }
}

// ─── Segmentación morfológica ─────────────────────────────────────────────────

fn find(mut i: u32, parent: &mut [u32]) -> u32 {
    while parent[i as usize] != i {
        parent[i as usize] = parent[parent[i as usize] as usize];
        i = parent[i as usize];
    }
    i
}

fn union(i: u32, j: u32, parent: &mut [u32]) {
    let root_i = find(i, parent);
    let root_j = find(j, parent);
    if root_i != root_j {
        parent[root_i as usize] = root_j;
    }
}

fn segment_ui_structure(buf: &[u32], w: usize, h: usize) -> (Vec<u32>, Vec<Rect>) {
    let mut binary_mask = vec![false; buf.len()];
    for y in 1..(h - 1) {
        for x in 1..(w - 1) {
            let idx = y * w + x;
            let luma = |p: u32| -> i32 {
                (((p >> 16) & 0xFF) as i32 + ((p >> 8) & 0xFF) as i32 + (p & 0xFF) as i32) / 3
            };
            let diff_h = (luma(buf[idx]) - luma(buf[idx + 1])).abs();
            let diff_v = (luma(buf[idx]) - luma(buf[idx + w])).abs();
            if diff_h > 15 || diff_v > 15 {
                binary_mask[idx] = true;
            }
        }
    }

    let mut smeared_mask = binary_mask.clone();
    let h_thresh = 35;
    let v_thresh = 14;

    for y in 0..h {
        let mut last_seen = None;
        for x in 0..w {
            let idx = y * w + x;
            if binary_mask[idx] {
                if let Some(lx) = last_seen {
                    if x - lx <= h_thresh {
                        for fill_x in lx..=x { smeared_mask[y * w + fill_x] = true; }
                    }
                }
                last_seen = Some(x);
            }
        }
    }

    let intermediate_mask = smeared_mask.clone();
    for x in 0..w {
        let mut last_seen = None;
        for y in 0..h {
            let idx = y * w + x;
            if intermediate_mask[idx] {
                if let Some(ly) = last_seen {
                    if y - ly <= v_thresh {
                        for fill_y in ly..=y { smeared_mask[fill_y * w + x] = true; }
                    }
                }
                last_seen = Some(y);
            }
        }
    }

    let mut labels = vec![0u32; buf.len()];
    let mut next_label = 1u32;
    let mut parent: Vec<u32> = (0..500_000).collect();

    for y in 1..h {
        for x in 1..w {
            let idx = y * w + x;
            if smeared_mask[idx] {
                let left_label = labels[idx - 1];
                let up_label   = labels[idx - w];

                match (left_label > 0, up_label > 0) {
                    (false, false) => {
                        if next_label >= parent.len() as u32 {
                            let old_len = parent.len();
                            parent.extend(old_len as u32..(old_len * 2) as u32);
                        }
                        labels[idx] = next_label;
                        next_label += 1;
                    }
                    (true, false) => { labels[idx] = left_label; }
                    (false, true) => { labels[idx] = up_label; }
                    (true, true)  => {
                        labels[idx] = left_label;
                        union(left_label, up_label, &mut parent);
                    }
                }
            }
        }
    }

    let mut max_used_label = 0u32;
    for idx in 0..labels.len() {
        if labels[idx] > 0 {
            let root = find(labels[idx], &mut parent);
            labels[idx] = root;
            if root > max_used_label { max_used_label = root; }
        }
    }

    let mut label_remap = vec![0u32; (max_used_label + 1) as usize];
    let mut real_idx = 1u32;
    let mut rects: Vec<Rect> = Vec::new();

    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            let old_l = labels[idx];
            if old_l > 0 {
                if label_remap[old_l as usize] == 0 {
                    label_remap[old_l as usize] = real_idx;
                    real_idx += 1;
                    rects.push(Rect { xmin: w, ymin: h, xmax: 0, ymax: 0 });
                }
                let target = (label_remap[old_l as usize] - 1) as usize;
                labels[idx] = label_remap[old_l as usize];
                let r = &mut rects[target];
                if x < r.xmin { r.xmin = x; }
                if x > r.xmax { r.xmax = x; }
                if y < r.ymin { r.ymin = y; }
                if y > r.ymax { r.ymax = y; }
            }
        }
    }

    for idx in 0..labels.len() {
        let l = labels[idx];
        if l > 0 {
            let r = rects[(l - 1) as usize];
            let too_small = (r.xmax < r.xmin || r.ymax < r.ymin)
                || ((r.xmax - r.xmin) <= 6 && (r.ymax - r.ymin) <= 6);
            if too_small { labels[idx] = 0; }
        }
    }
    (labels, rects)
}

// ─── Pipeline de segmentación (Etapa 1 y Etapa 2 integrada) ──────────────────

fn fh_segment(buf: &[u32], w: usize, h: usize) -> (Vec<u32>, Vec<Region>) {
    let (label_map, rects) = segment_ui_structure(buf, w, h);
    let n = rects.len();

    let mut area:    Vec<usize> = vec![0;   n];
    let mut sum_r:   Vec<f64>   = vec![0.0; n];
    let mut sum_g:   Vec<f64>   = vec![0.0; n];
    let mut sum_b:   Vec<f64>   = vec![0.0; n];
    let mut sum_r2:  Vec<f64>   = vec![0.0; n];
    let mut sum_g2:  Vec<f64>   = vec![0.0; n];
    let mut sum_b2:  Vec<f64>   = vec![0.0; n];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            let lbl = label_map[idx];
            if lbl == 0 { continue; }
            let rid = (lbl - 1) as usize;

            let p = buf[idx];
            let r = ((p >> 16) & 0xFF) as f64;
            let g = ((p >>  8) & 0xFF) as f64;
            let b = ( p        & 0xFF) as f64;

            area[rid]   += 1;
            sum_r[rid]  += r;   sum_r2[rid] += r * r;
            sum_g[rid]  += g;   sum_g2[rid] += g * g;
            sum_b[rid]  += b;   sum_b2[rid] += b * b;

            if x + 1 < w {
                let rbl = label_map[idx + 1];
                if rbl > 0 && rbl != lbl {
                    let nid = (rbl - 1) as usize;
                    adj[rid].push(nid);
                    adj[nid].push(rid);
                }
            }
            if y + 1 < h {
                let dbl = label_map[idx + w];
                if dbl > 0 && dbl != lbl {
                    let nid = (dbl - 1) as usize;
                    adj[rid].push(nid);
                    adj[nid].push(rid);
                }
            }
        }
    }

    let regions = rects
        .into_iter()
        .enumerate()
        .map(|(id, bbox)| {
            let a = area[id].max(1) as f64;
            let mr = (sum_r[id] / a) as f32;
            let mg = (sum_g[id] / a) as f32;
            let mb = (sum_b[id] / a) as f32;

            let vr = (sum_r2[id] / a) - (sum_r[id] / a).powi(2);
            let vg = (sum_g2[id] / a) - (sum_g[id] / a).powi(2);
            let vb = (sum_b2[id] / a) - (sum_b[id] / a).powi(2);
            let variance = ((vr + vg + vb) / 3.0) as f32;

            let mut neighbors = adj[id].clone();
            neighbors.sort_unstable();
            neighbors.dedup();

            Region {
                id,
                bbox,
                area: area[id],
                perimeter: 0,
                mean_color: (mr, mg, mb),
                color_variance: variance,
                neighbors,
            }
        })
        .collect();

    (label_map, regions)
}

fn merge_regions(
    label_map: &mut [u32],
    regions:   Vec<Region>,
    _buf:      &[u32],
    w:         usize,
    h:         usize,
) -> Vec<Region> {
    let n = regions.len();
    if n == 0 { return regions; }

    let mut edges: Vec<MergeEdge> = Vec::with_capacity(n * 4);
    let mut seen_pairs: std::collections::HashSet<(usize, usize)> =
        std::collections::HashSet::with_capacity(n * 4);

    for reg in &regions {
        for &nb in &reg.neighbors {
            if nb >= n { continue; }
            let (lo, hi) = if reg.id < nb { (reg.id, nb) } else { (nb, reg.id) };
            if seen_pairs.insert((lo, hi)) {
                let cost = merge_cost(&regions[lo], &regions[hi], w, h);
                edges.push(MergeEdge { cost, a: lo, b: hi });
            }
        }
    }

    edges.sort_unstable_by(|e1, e2| e1.cost.partial_cmp(&e2.cost).unwrap_or(std::cmp::Ordering::Equal));

    let mut dsu = Dsu::new(n);
    let mut comp_area: Vec<usize> = regions.iter().map(|r| r.area.max(1)).collect();
    let mut comp_mint: Vec<f32> = regions.iter().map(|r| mint(r.area)).collect();

    for edge in &edges {
        let ra = dsu.find(edge.a);
        let rb = dsu.find(edge.b);
        if ra == rb { continue; }

        let threshold = comp_mint[ra].min(comp_mint[rb]);
        if edge.cost < threshold {
            dsu.union(ra, rb);
            let new_root = dsu.find(ra);
            comp_area[new_root] = comp_area[ra] + comp_area[rb];
            comp_mint[new_root] = comp_mint[ra].max(comp_mint[rb]).max(edge.cost);
        }
    }

    let mut root_to_new: Vec<u32> = vec![u32::MAX; n];
    let mut new_id_counter: u32 = 0;

    let mut new_bboxes:  Vec<Rect>           = Vec::with_capacity(n / 2 + 1);
    let mut new_areas:   Vec<usize>          = Vec::with_capacity(n / 2 + 1);
    let mut new_sum_r:   Vec<f64>            = Vec::with_capacity(n / 2 + 1);
    let mut new_sum_g:   Vec<f64>            = Vec::with_capacity(n / 2 + 1);
    let mut new_sum_b:   Vec<f64>            = Vec::with_capacity(n / 2 + 1);
    let mut new_variance:Vec<f32>            = Vec::with_capacity(n / 2 + 1);

    let mut old_to_new: Vec<u32> = vec![0; n];

    for reg in &regions {
        let root = dsu.find(reg.id);
        let nid = if root_to_new[root] == u32::MAX {
            let id = new_id_counter;
            root_to_new[root] = id;
            new_id_counter += 1;
            new_bboxes.push(reg.bbox);
            new_areas.push(0);
            new_sum_r.push(0.0);
            new_sum_g.push(0.0);
            new_sum_b.push(0.0);
            new_variance.push(0.0);
            id
        } else {
            root_to_new[root]
        };

        old_to_new[reg.id] = nid;

        let ni = nid as usize;
        new_bboxes[ni] = new_bboxes[ni].union_with(reg.bbox);
        let a = reg.area as f64;
        new_areas[ni]  += reg.area;
        new_sum_r[ni]  += reg.mean_color.0 as f64 * a;
        new_sum_g[ni]  += reg.mean_color.1 as f64 * a;
        new_sum_b[ni]  += reg.mean_color.2 as f64 * a;
        new_variance[ni] = (new_variance[ni] * (new_areas[ni] - reg.area) as f32
            + reg.color_variance * reg.area as f32)
            / new_areas[ni] as f32;
    }

    let num_merged = new_id_counter as usize;

    let mut merged_regions: Vec<Region> = (0..num_merged).map(|i| {
        let a = new_areas[i].max(1) as f64;
        Region {
            id:             i,
            bbox:           new_bboxes[i],
            area:           new_areas[i],
            perimeter:      0,
            mean_color: (
                (new_sum_r[i] / a) as f32,
                (new_sum_g[i] / a) as f32,
                (new_sum_b[i] / a) as f32,
            ),
            color_variance: new_variance[i],
            neighbors: Vec::new(),
        }
    }).collect();

    for lbl in label_map.iter_mut() {
        if *lbl > 0 {
            let old_0 = (*lbl - 1) as usize;
            if old_0 < n {
                *lbl = old_to_new[old_0] + 1;
            }
        }
    }

    {
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); num_merged];
        for edge in &edges {
            let na = old_to_new[edge.a] as usize;
            let nb = old_to_new[edge.b] as usize;
            if na != nb {
                adj[na].push(nb);
                adj[nb].push(na);
            }
        }
        for (i, nbrs) in adj.into_iter().enumerate() {
            let mut v = nbrs;
            v.sort_unstable();
            v.dedup();
            merged_regions[i].neighbors = v;
        }
    }

    merged_regions
}

fn build_region_tree(regions: &[Region]) -> Vec<Rect> {
    regions.iter().map(|r| r.bbox).collect()
}

// ─── HintMode: pipeline de datos ──────────────────────────────────────────────

const HINT_MIN_AREA: usize       = 400;
const HINT_MIN_DIMENSION: usize  = 8;
const HINT_MAX_ASPECT_RATIO: f32 = 20.0;

const HINT_KEYS: &[Key] = &[
    Key::A, Key::S, Key::D, Key::G, Key::H, Key::J, Key::K, Key::L,
    Key::Q, Key::W, Key::E, Key::R, Key::T, Key::Y, Key::U, Key::I, Key::O, Key::P,
    Key::Z, Key::X, Key::C, Key::V, Key::B, Key::N, Key::M,
];

fn key_to_char(key: Key) -> Option<char> {
    match key {
        Key::A => Some('A'), Key::B => Some('B'), Key::C => Some('C'),
        Key::D => Some('D'), Key::E => Some('E'),
        Key::G => Some('G'), Key::H => Some('H'), Key::I => Some('I'),
        Key::J => Some('J'), Key::K => Some('K'), Key::L => Some('L'),
        Key::M => Some('M'), Key::N => Some('N'), Key::O => Some('O'),
        Key::P => Some('P'), Key::Q => Some('Q'), Key::R => Some('R'),
        Key::S => Some('S'), Key::T => Some('T'), Key::U => Some('U'),
        Key::V => Some('V'), Key::W => Some('W'), Key::X => Some('X'),
        Key::Y => Some('Y'), Key::Z => Some('Z'),
        _ => None,
    }
}

fn find_hint_by_label(hints: &[Hint], label: &str) -> Option<usize> {
    hints.iter().find(|h| h.label == label).map(|h| h.candidate_index)
}

fn filter_noise(blocks: &[Rect], _screen_width: usize, _screen_height: usize) -> Vec<usize> {
    blocks.iter().enumerate().filter_map(|(i, r)| {
        if r.xmax < r.xmin || r.ymax < r.ymin { return None; }
        let bw = r.xmax - r.xmin + 1;
        let bh = r.ymax - r.ymin + 1;
        let area = bw * bh;
        if area < HINT_MIN_AREA || bw < HINT_MIN_DIMENSION || bh < HINT_MIN_DIMENSION { return None; }
        let aspect = (bw as f32 / bh as f32).max(bh as f32 / bw as f32);
        if aspect > HINT_MAX_ASPECT_RATIO { return None; }
        Some(i)
    }).collect()
}

fn rank_candidates(valid_indices: &[usize], blocks: &[Rect], cursor_x: usize, cursor_y: usize) -> Vec<Candidate> {
    if valid_indices.is_empty() { return Vec::new(); }
    let weights = CandidateWeights::default();
    let max_area: f32 = valid_indices.iter().map(|&i| {
        let r = &blocks[i]; ((r.xmax - r.xmin + 1) * (r.ymax - r.ymin + 1)) as f32
    }).fold(1.0_f32, f32::max);
    let max_dist: f32 = valid_indices.iter().map(|&i| {
        let r = &blocks[i];
        let cx = (r.xmin + r.xmax) / 2;
        let cy = (r.ymin + r.ymax) / 2;
        let dx = cx as f32 - cursor_x as f32;
        let dy = cy as f32 - cursor_y as f32;
        (dx * dx + dy * dy).sqrt()
    }).fold(1.0_f32, f32::max);

    let max_aspect: f32 = HINT_MAX_ASPECT_RATIO;
    let mut candidates: Vec<Candidate> = valid_indices.iter().map(|&i| {
        let r = &blocks[i];
        let bw = (r.xmax - r.xmin + 1) as f32;
        let bh = (r.ymax - r.ymin + 1) as f32;
        let center_x = (r.xmin + r.xmax) / 2;
        let center_y = (r.ymin + r.ymax) / 2;
        let dx = center_x as f32 - cursor_x as f32;
        let dy = center_y as f32 - cursor_y as f32;
        let dist = (dx * dx + dy * dy).sqrt();
        let area_norm   = (bw * bh) / max_area;
        let dist_norm   = 1.0 - (dist / max_dist);
        let aspect      = (bw / bh).max(bh / bw);
        let aspect_norm = 1.0 - ((aspect - 1.0) / (max_aspect - 1.0)).min(1.0);
        let metrics = CandidateMetrics { area: area_norm, distance: dist_norm, aspect_ratio: aspect_norm };
        let total_score = metrics.area * weights.area + metrics.distance * weights.distance + metrics.aspect_ratio * weights.aspect_ratio;
        Candidate { id: CandidateId(i), center_x, center_y, metrics, total_score }
    }).collect();
    candidates.sort_by(|a, b| b.total_score.partial_cmp(&a.total_score).unwrap_or(std::cmp::Ordering::Equal));
    candidates
}

fn generate_hints(candidates: &[Candidate]) -> Vec<Hint> {
    const HINT_LABELS: &[char] = &[
        'A','S','D','G','H','J','K','L',
        'Q','W','E','R','T','Y','U','I','O','P',
        'Z','X','C','V','B','N','M',
    ];
    let labels: Vec<String> = HINT_LABELS.iter()
        .flat_map(|&a| HINT_LABELS.iter().map(move |&b| format!("{}{}", a, b)))
        .collect();
    candidates.iter()
        .enumerate()
        .take(labels.len())
        .map(|(i, _)| Hint {
            label:           labels[i].clone(),
            candidate_index: i,
        })
        .collect()
}

// ─── Fuente bitmap 5×7 para HintMode ─────────────────────────────────────────

const FONT_5X7: &[(char, [u8; 7])] = &[
    ('A', [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('B', [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110]),
    ('C', [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110]),
    ('D', [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110]),
    ('E', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111]),
    ('F', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('G', [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111]),
    ('H', [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('I', [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('J', [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100]),
    ('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
    ('L', [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
    ('M', [0b10001, 0b11011, 0b10101, 0b10001, 0b10001, 0b10001, 0b10001]),
    ('N', [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001]),
    ('O', [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('P', [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('Q', [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101]),
    ('R', [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001]),
    ('S', [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110]),
    ('T', [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('U', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('V', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
    ('W', [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001]),
    ('X', [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001]),
    ('Y', [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('Z', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111]),
];

const GLYPH_W: usize = 5;
const GLYPH_H: usize = 7;
const HINT_MARGIN: usize = 2;
const LABEL_W: usize = GLYPH_W + HINT_MARGIN * 2;
const LABEL_H: usize = GLYPH_H + HINT_MARGIN * 2;
const HINT_BG_COLOR:   u32 = 0x000000;
const HINT_TEXT_COLOR: u32 = 0xFFFF00;

fn glyph_row(ch: char, row: usize) -> u8 {
    for &(c, ref bitmap) in FONT_5X7 {
        if c == ch { return bitmap[row]; }
    }
    0
}

const GLYPH_GAP: usize = 1;
const LABEL2_W: usize = GLYPH_W * 2 + GLYPH_GAP + HINT_MARGIN * 2;

fn draw_hints(
    buf: &mut [u32],
    w: usize,
    h: usize,
    hints: &[Hint],
    candidates: &[Candidate],
) {
    const MAX_HINTS: usize = 10;
    for hint in hints.iter().take(MAX_HINTS) {
        let candidate = &candidates[hint.candidate_index];
        let cx = candidate.center_x;
        let cy = candidate.center_y;

        let box_x = cx.saturating_sub(LABEL2_W / 2);
        let box_y = cy.saturating_sub(LABEL_H / 2);

        let box_x2 = (box_x + LABEL2_W).min(w);
        let box_y2 = (box_y + LABEL_H).min(h);

        for py in box_y..box_y2 {
            for px in box_x..box_x2 {
                buf[py * w + px] = HINT_BG_COLOR;
            }
        }

        let glyph_y0 = box_y + HINT_MARGIN;
        let chars: Vec<char> = hint.label.chars().collect();

        for (gi, &ch) in chars.iter().enumerate() {
            let glyph_x0 = box_x + HINT_MARGIN + gi * (GLYPH_W + GLYPH_GAP);
            for row in 0..GLYPH_H {
                let bits = glyph_row(ch, row);
                let py = glyph_y0 + row;
                if py >= h { break; }
                for col in 0..GLYPH_W {
                    if (bits >> (GLYPH_W - 1 - col)) & 1 == 1 {
                        let px = glyph_x0 + col;
                        if px < w {
                            buf[py * w + px] = HINT_TEXT_COLOR;
                        }
                    }
                }
            }
        }
    }
}

// ─── Render auxiliar ──────────────────────────────────────────────────────────

fn restore_bounding_box(src: &[u32], dest: &mut [u32], w: usize, rect: Rect) {
    for row in rect.ymin..=rect.ymax {
        let start = row * w + rect.xmin;
        let end   = row * w + rect.xmax + 1;
        if end <= src.len() { dest[start..end].copy_from_slice(&src[start..end]); }
    }
}

fn draw_rect_border(buf: &mut [u32], w: usize, h: usize, rect: Rect, color: u32) {
    for i in rect.xmin..=rect.xmax {
        if i < w {
            if rect.ymin < h { buf[rect.ymin * w + i] = color; }
            if rect.ymax < h { buf[rect.ymax * w + i] = color; }
        }
    }
    for i in rect.ymin..=rect.ymax {
        if i < h {
            if rect.xmin < w { buf[i * w + rect.xmin] = color; }
            if rect.xmax < w { buf[i * w + rect.xmax] = color; }
        }
    }
}

fn draw_cross_cursor(buf: &mut [u32], w: usize, h: usize, cx: usize, cy: usize, color: u32) {
    for i in -25i32..=25 {
        let nx = cx as i32 + i;
        let ny = cy as i32 + i;
        if nx >= 0 && nx < w as i32 { buf[cy * w + nx as usize] = color; }
        if ny >= 0 && ny < h as i32 { buf[ny as usize * w + cx]  = color; }
    }
}

fn draw_hint_borders(buf: &mut [u32], w: usize, h: usize, candidates: &[Candidate], blocks: &[Rect]) -> Option<Rect> {
    const HINT_COLOR: u32   = 0x444444;
    const MAX_HINTS: usize  = 10;
    let mut bounding: Option<Rect> = None;
    for candidate in candidates.iter().take(MAX_HINTS) {
        let rect = blocks[candidate.id.0].clamp_to_screen(w, h);
        draw_rect_border(buf, w, h, rect, HINT_COLOR);
        bounding = Some(match bounding {
            Some(b) => b.union_with(rect),
            None    => rect,
        });
    }
    bounding
}

// ─── Captura y copia al portapapeles ──────────────────────────────────────────

fn save_and_copy_sturdy(clean_buffer: &[u32], w: usize, area: (usize, usize, usize, usize)) {
    let (xmin, ymin, xmax, ymax) = area;
    let (cw, ch) = (xmax.saturating_sub(xmin) + 1, ymax.saturating_sub(ymin) + 1);
    if cw == 0 || ch == 0 { return; }

    let mut out = RgbaImage::new(cw as u32, ch as u32);
    for py in 0..ch {
        for px in 0..cw {
            let p = clean_buffer[(ymin + py) * w + (xmin + px)];
            out.put_pixel(px as u32, py as u32, image::Rgba([
                ((p >> 16) & 0xFF) as u8,
                ((p >> 8)  & 0xFF) as u8,
                ( p        & 0xFF) as u8,
                255,
            ]));
        }
    }

    let mut png_data: Vec<u8> = Vec::new();
    out.write_to(
        &mut std::io::Cursor::new(&mut png_data),
        image::ImageFormat::Png,
    ).expect("Error al codificar PNG");

    let mut child = Command::new("wl-copy")
        .args(["--type", "image/png"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Error al lanzar wl-copy");

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&png_data);
    }
}
