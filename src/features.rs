// ─── Etapa 3: Feature Extraction ─────────────────────────────────────────────
//
// Extrae features normalizadas por Region para alimentar el clasificador.
// Todas las features están en [0.0, 1.0] salvo aspect_ratio (>= 1.0).

use crate::Region;

/// Features derivadas de una Region, listas para clasificación.
#[derive(Clone, Debug)]
pub struct RegionFeatures {
    /// Fracción del área total de la pantalla ocupada por la región [0, 1]
    pub area_ratio: f32,
    /// max(w/h, h/w) — siempre >= 1.0
    pub aspect_ratio: f32,
    /// Varianza de color normalizada a [0, 1] (src: Region.color_variance / 65025)
    pub color_variance_norm: f32,
    /// Heurístico de densidad de bordes: razón perimeter² / area [0, 1 aprox]
    pub edge_density: f32,
    /// Centro X normalizado [0, 1]
    pub center_x_norm: f32,
    /// Centro Y normalizado [0, 1]
    pub center_y_norm: f32,
    /// Ancho normalizado por ancho de pantalla [0, 1]
    pub width_norm: f32,
    /// Alto normalizado por alto de pantalla [0, 1]
    pub height_norm: f32,
    /// Número de vecinos (adyacencias de región), no normalizado
    pub neighbor_count: usize,
}

impl RegionFeatures {
    pub fn new(region: &Region, screen_w: usize, screen_h: usize) -> Self {
        let screen_area = (screen_w * screen_h).max(1) as f32;
        let area_ratio = region.area as f32 / screen_area;

        let bbox_w = (region.bbox.xmax.saturating_sub(region.bbox.xmin) + 1) as f32;
        let bbox_h = (region.bbox.ymax.saturating_sub(region.bbox.ymin) + 1) as f32;
        
        let aspect_ratio = (bbox_w / bbox_h).max(bbox_h / bbox_w);

        // La varianza máxima teórica en u8 es 255² = 65025.0
        let color_variance_norm = (region.color_variance / 65025.0).clamp(0.0, 1.0);

        // ── Densidad de Bordes / Rugosidad Formológica ─────────────────────
        // En una región perfectamente sólida: perimeter = 2*(w+h).
        // Para regiones muy irregulares el perímetro crece mucho respecto al área.
        // Usamos isoperimetric ratio: p² / (4π·area), saturado a [0,1].
        // Valores altos → borde muy irregular → probable zona de texto/icono.
        let p = region.perimeter as f32;
        let a = region.area.max(1) as f32;
        let iso = (p * p) / (4.0 * std::f32::consts::PI * a);
        // iso = 1 para círculo perfecto, crece sin límite para formas complejas.
        // Mapeamos con saturación: iso > 10 → edge_density ≈ 1
        let edge_density = (iso / 10.0).clamp(0.0, 1.0);

        // ── Posición centro normalizada ────────────────────────────────────
        let cx = (region.bbox.xmin + region.bbox.xmax) as f32 / 2.0;
        let cy = (region.bbox.ymin + region.bbox.ymax) as f32 / 2.0;
        let center_x_norm = (cx / screen_w as f32).clamp(0.0, 1.0);
        let center_y_norm = (cy / screen_h as f32).clamp(0.0, 1.0);

        // ── Tamaño normalizado ────────────────────────────────────────────
        let width_norm  = (bbox_w / screen_w as f32).clamp(0.0, 1.0);
        let height_norm = (bbox_h / screen_h as f32).clamp(0.0, 1.0);

        RegionFeatures {
            area_ratio,
            aspect_ratio,
            color_variance_norm,
            edge_density,
            center_x_norm,
            center_y_norm,
            width_norm,
            height_norm,
            neighbor_count: region.neighbors.len(),
        }
    }
}