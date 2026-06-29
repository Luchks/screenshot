// ─── Etapa 3: Clasificador Semántico Heurístico ───────────────────────────────
//
// Sistema de scoring determinístico por tipo de elemento UI.
// Cada tipo acumula evidencia a través de sus reglas propias.
// No hay ML: todo es aritmética sobre features normalizadas.

use crate::features::RegionFeatures;

/// Tipo semántico inferido de una región.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UIElementType {
    /// Elemento interactivo pequeño y compacto con buen contraste
    Button,
    /// Zona de texto: alta densidad de bordes, aspecto horizontal, color uniforme
    Text,
    /// Contenedor de gran área, color uniforme, muchos vecinos
    Panel,
    /// Zona pictórica: varianza de color alta, forma cuadrada-ish
    Image,
    /// Input: horizontal, área pequeña-media, borde definido
    Input,
    /// No hay evidencia suficiente para ningún tipo
    Unknown,
}

impl UIElementType {
    pub fn label(&self) -> &'static str {
        match self {
            UIElementType::Button  => "Button",
            UIElementType::Text    => "Text",
            UIElementType::Panel   => "Panel",
            UIElementType::Image   => "Image",
            UIElementType::Input   => "Input",
            UIElementType::Unknown => "Unknown",
        }
    }
}

/// Resultado de clasificación de una región.
#[derive(Clone, Debug)]
pub struct ClassificationResult {
    pub element_type:    UIElementType,
    /// Score del tipo ganador en [0, 1]
    pub confidence:      f32,
    /// Importancia relativa del elemento en el contexto de la pantalla [0, 1]
    pub importance_score: f32,
}

// ── Reglas por tipo ───────────────────────────────────────────────────────────
//
// Cada función devuelve un score crudo en [0, 1] representando cuánto se parece
// la región a ese tipo. Los scores se comparan directamente.

fn score_button(f: &RegionFeatures) -> f32 {
    let mut score = 0.0_f32;

    // Botones son pequeños o medianos (< 8% pantalla)
    if f.area_ratio < 0.08 {
        score += 0.25 * (1.0 - f.area_ratio / 0.08);
    }

    // Aspecto relativamente cuadrado o ligeramente horizontal (1..3)
    if f.aspect_ratio <= 3.0 {
        score += 0.25 * (1.0 - (f.aspect_ratio - 1.0) / 2.0).max(0.0);
    }

    // Varianza de color baja-media: botones tienen color sólido
    if f.color_variance_norm < 0.15 {
        score += 0.20 * (1.0 - f.color_variance_norm / 0.15);
    }

    // Densidad de borde media (tiene contorno pero no es texto)
    let edge_match = 1.0 - (f.edge_density - 0.3).abs() / 0.3;
    score += 0.15 * edge_match.max(0.0);

    // Pocos vecinos directos (elemento aislado)
    if f.neighbor_count <= 4 {
        score += 0.15;
    }

    score.clamp(0.0, 1.0)
}

fn score_text(f: &RegionFeatures) -> f32 {
    let mut score = 0.0_f32;

    // Texto es pequeño o mediano
    if f.area_ratio < 0.15 {
        score += 0.15 * (1.0 - f.area_ratio / 0.15);
    }

    // Texto es horizontal (aspecto > 2)
    if f.aspect_ratio > 2.0 {
        score += 0.30 * ((f.aspect_ratio - 2.0) / 8.0).min(1.0);
    }

    // Color muy uniforme (texto sobre fondo sólido)
    if f.color_variance_norm < 0.08 {
        score += 0.25 * (1.0 - f.color_variance_norm / 0.08);
    }

    // Alta densidad de bordes (glifos = muchos contornos)
    score += 0.30 * f.edge_density;

    score.clamp(0.0, 1.0)
}

fn score_panel(f: &RegionFeatures) -> f32 {
    let mut score = 0.0_f32;

    // Panel es grande (> 10% pantalla)
    if f.area_ratio > 0.10 {
        score += 0.35 * ((f.area_ratio - 0.10) / 0.50).min(1.0);
    }

    // Color muy uniforme (fondo sólido)
    if f.color_variance_norm < 0.05 {
        score += 0.25 * (1.0 - f.color_variance_norm / 0.05);
    }

    // Muchos vecinos (contenedor de muchos elementos)
    if f.neighbor_count > 3 {
        score += 0.20 * ((f.neighbor_count - 3) as f32 / 10.0).min(1.0);
    }

    // Baja densidad de borde (forma regular)
    score += 0.20 * (1.0 - f.edge_density);

    score.clamp(0.0, 1.0)
}

fn score_image(f: &RegionFeatures) -> f32 {
    let mut score = 0.0_f32;

    // Imágenes tienen alta varianza de color
    if f.color_variance_norm > 0.10 {
        score += 0.40 * ((f.color_variance_norm - 0.10) / 0.40).min(1.0);
    }

    // Aspecto cercano a cuadrado o 4:3 / 16:9 (1..2.5)
    if f.aspect_ratio <= 2.5 {
        score += 0.25 * (1.0 - (f.aspect_ratio - 1.0) / 1.5).max(0.0);
    }

    // Tamaño mediano (no minúscula, no pantalla completa)
    let size_match = 1.0 - (f.area_ratio - 0.10).abs() / 0.10;
    score += 0.20 * size_match.max(0.0);

    // Densidad de borde media-baja (imagen tiene contorno pero no interior irregular)
    if f.edge_density < 0.4 {
        score += 0.15 * (1.0 - f.edge_density / 0.4);
    }

    score.clamp(0.0, 1.0)
}

fn score_input(f: &RegionFeatures) -> f32 {
    let mut score = 0.0_f32;

    // Input es pequeño-mediano
    if f.area_ratio < 0.06 {
        score += 0.20 * (1.0 - f.area_ratio / 0.06);
    }

    // Muy horizontal (aspecto 3..8)
    if f.aspect_ratio >= 3.0 && f.aspect_ratio <= 10.0 {
        score += 0.35 * (1.0 - (f.aspect_ratio - 5.0).abs() / 5.0).max(0.0);
    }

    // Color muy uniforme (campo vacío)
    if f.color_variance_norm < 0.06 {
        score += 0.25 * (1.0 - f.color_variance_norm / 0.06);
    }

    // Borde definido (density baja-media)
    let edge_match = 1.0 - (f.edge_density - 0.2).abs() / 0.2;
    score += 0.20 * edge_match.max(0.0);

    score.clamp(0.0, 1.0)
}

// ── Importance score ──────────────────────────────────────────────────────────
//
// Combina tamaño, posición (centrado = más importante), contraste y tipo.

fn compute_importance(f: &RegionFeatures, element_type: UIElementType, confidence: f32) -> f32 {
    // Tamaño normalizado con saturación
    let size_score = (f.area_ratio * 5.0).min(1.0);

    // Centralidad: distancia al centro de pantalla
    let dx = f.center_x_norm - 0.5;
    let dy = f.center_y_norm - 0.5;
    let centrality = 1.0 - (dx * dx + dy * dy).sqrt() / 0.707;

    // Contraste visual (varianza de color)
    let contrast = (f.color_variance_norm * 3.0).min(1.0);

    // Bonus por tipo interactivo
    let type_bonus: f32 = match element_type {
        UIElementType::Button | UIElementType::Input => 0.15,
        UIElementType::Text                          => 0.10,
        UIElementType::Panel                         => 0.05,
        UIElementType::Image                         => 0.08,
        UIElementType::Unknown                       => 0.0,
    };

    let raw = 0.30 * size_score
        + 0.30 * centrality
        + 0.20 * contrast
        + 0.20 * confidence
        + type_bonus;

    raw.clamp(0.0, 1.0)
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Clasifica una región dado su vector de features.
/// Devuelve tipo, confianza e importancia.
pub fn classify_region(f: &RegionFeatures) -> ClassificationResult {
    let scores: [(UIElementType, f32); 5] = [
        (UIElementType::Button, score_button(f)),
        (UIElementType::Text,   score_text(f)),
        (UIElementType::Panel,  score_panel(f)),
        (UIElementType::Image,  score_image(f)),
        (UIElementType::Input,  score_input(f)),
    ];

    // Ganador por score máximo
    let (best_type, best_score) = scores
        .iter()
        .copied()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or((UIElementType::Unknown, 0.0));

    // Umbral mínimo de confianza para no reportar Unknown
    let (element_type, confidence) = if best_score >= 0.18 {
        (best_type, best_score)
    } else {
        (UIElementType::Unknown, best_score)
    };

    let importance_score = compute_importance(f, element_type, confidence);

    ClassificationResult { element_type, confidence, importance_score }
}
