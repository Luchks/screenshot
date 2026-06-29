// ─── Etapa 4: Motor de Interacción Semántica ─────────────────────────────────
//
// Capa de interacción sobre UIHierarchyTree. No modifica structs existentes.
// No hay ML. Todo es aritmética sobre campos ya presentes en UIElement.
//
// Uso mínimo desde main.rs:
//   let engine = InteractionEngine::new();
//   let target = engine.compute_smart_snap(&tree, cursor_x, cursor_y);
//   let action = engine.infer_action(element_type);

use crate::Rect;
use crate::classify::UIElementType;
use crate::hierarchy::{UIElement, UIHierarchyTree};

// ─── UIAction ────────────────────────────────────────────────────────────────
//
// Acción semántica inferida desde el tipo de elemento.
// No requiere contexto adicional: es una deducción 1:1 desde UIElementType.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UIAction {
    /// Click en botón interactivo
    Click,
    /// Seleccionar/copiar zona de texto
    SelectText,
    /// Enfocar y tipear en campo de entrada
    FocusInput,
    /// Expandir / colapsar panel contenedor
    ExpandPanel,
    /// Ver imagen (zoom, previsualizar)
    ViewImage,
    /// Sin acción semántica determinada
    NoAction,
}

impl UIAction {
    pub fn label(&self) -> &'static str {
        match self {
            UIAction::Click       => "Click",
            UIAction::SelectText  => "SelectText",
            UIAction::FocusInput  => "FocusInput",
            UIAction::ExpandPanel => "ExpandPanel",
            UIAction::ViewImage   => "ViewImage",
            UIAction::NoAction    => "NoAction",
        }
    }
}

// ─── SelectionTarget ─────────────────────────────────────────────────────────
//
// Resultado de una operación de selección: puede apuntar a un UIElement
// (interacción semántica completa) o a un Rect fallback (compatibilidad con
// el pipeline actual basado en bloques).

#[derive(Clone, Debug)]
pub enum SelectionTarget {
    /// Elemento semántico identificado por su id en UIHierarchyTree.elements
    Element(usize),
    /// Fallback geométrico: solo bounding box, sin semántica
    Region(Rect),
}

// ─── InteractionEngine ───────────────────────────────────────────────────────

pub struct InteractionEngine;

impl InteractionEngine {
    pub fn new() -> Self {
        InteractionEngine
    }

    // ── infer_action ─────────────────────────────────────────────────────────
    //
    // Mapeo determinístico UIElementType → UIAction.
    // No requiere contexto de árbol ni features: es puramente estructural.

    pub fn infer_action(&self, element_type: UIElementType) -> UIAction {
        match element_type {
            UIElementType::Button  => UIAction::Click,
            UIElementType::Text    => UIAction::SelectText,
            UIElementType::Input   => UIAction::FocusInput,
            UIElementType::Panel   => UIAction::ExpandPanel,
            UIElementType::Image   => UIAction::ViewImage,
            UIElementType::Unknown => UIAction::NoAction,
        }
    }

    // ── find_next_target ─────────────────────────────────────────────────────
    //
    // Devuelve el siguiente UIElement para navegación secuencial (tab/flechas).
    //
    // Algoritmo:
    //   1. Filtra por tipo si se pasa Some(UIElementType).
    //   2. Ordena todos los candidatos por importance_score descendente.
    //   3. Si hay current_focus, devuelve el siguiente en ese orden circular.
    //   4. Si no hay foco actual, devuelve el de mayor importancia.
    //
    // No inventa campos: solo usa `id` e `importance_score` existentes.

    pub fn find_next_target(
        &self,
        tree: &UIHierarchyTree,
        current_focus: Option<usize>,
        filter_type: Option<UIElementType>,
    ) -> Option<SelectionTarget> {
        // Filtrar y ordenar por importance_score descendente
        let mut candidates: Vec<&UIElement> = tree.elements.iter()
            .filter(|el| {
                filter_type.map_or(true, |t| el.element_type == t)
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        candidates.sort_by(|a, b| {
            b.importance_score
                .partial_cmp(&a.importance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        match current_focus {
            None => {
                // Sin foco: devolver el de mayor importancia
                Some(SelectionTarget::Element(candidates[0].id))
            }
            Some(focus_id) => {
                // Encontrar posición del foco actual y avanzar circularmente
                let pos = candidates.iter().position(|el| el.id == focus_id);
                let next_idx = match pos {
                    Some(i) => (i + 1) % candidates.len(),
                    None    => 0, // foco no está en la lista filtrada, ir al primero
                };
                Some(SelectionTarget::Element(candidates[next_idx].id))
            }
        }
    }

    // ── compute_smart_snap ───────────────────────────────────────────────────
    //
    // Dado un cursor (cursor_x, cursor_y), encuentra el UIElement más relevante
    // para hacer snap semántico.
    //
    // Scoring por elemento:
    //   snap_score = distance / (importance_score + ε)
    //
    // El elemento con menor snap_score gana: bajo score significa
    // cercano Y/O importante. Un elemento muy importante puede ganar incluso
    // estando un poco más lejos que uno irrelevante.
    //
    // Centroide calculado desde region.bbox (campos existentes).
    // Fallback a SelectionTarget::Region con el bbox del ganador si el
    // confidence es 0 (UIElementType::Unknown sin confianza).

    pub fn compute_smart_snap(
        &self,
        tree: &UIHierarchyTree,
        cursor_x: usize,
        cursor_y: usize,
    ) -> Option<SelectionTarget> {
        if tree.elements.is_empty() {
            return None;
        }

        let cx = cursor_x as f32;
        let cy = cursor_y as f32;

        let best = tree.elements.iter().min_by(|a, b| {
            let score_a = snap_score(a, cx, cy);
            let score_b = snap_score(b, cx, cy);
            score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        best.map(|el| {
            if el.element_type == UIElementType::Unknown && el.confidence == 0.0 {
                // Fallback geométrico puro
                SelectionTarget::Region(el.region.bbox)
            } else {
                SelectionTarget::Element(el.id)
            }
        })
    }

    // ── snap_to_element ──────────────────────────────────────────────────────
    //
    // Convierte un SelectionTarget::Element en el Rect de su bbox.
    // Útil para compatibilidad con el pipeline de render actual sin
    // modificar main.rs.

    pub fn resolve_rect(&self, tree: &UIHierarchyTree, target: &SelectionTarget) -> Option<Rect> {
        match target {
            SelectionTarget::Element(id) => {
                tree.elements.get(*id).map(|el| el.region.bbox)
            }
            SelectionTarget::Region(rect) => Some(*rect),
        }
    }

    // ── element_at ───────────────────────────────────────────────────────────
    //
    // Dado un punto (x, y), devuelve el UIElement más pequeño cuyo bbox
    // lo contiene (el más específico / menos padre).
    // Útil para hit-testing semántico en hover/click.

    pub fn element_at(
        &self,
        tree: &UIHierarchyTree,
        x: usize,
        y: usize,
    ) -> Option<SelectionTarget> {
        // Todos los elementos que contienen el punto
        let mut hits: Vec<&UIElement> = tree.elements.iter()
            .filter(|el| {
                let b = &el.region.bbox;
                x >= b.xmin && x <= b.xmax && y >= b.ymin && y <= b.ymax
            })
            .collect();

        if hits.is_empty() {
            return None;
        }

        // El más pequeño en área = el más específico semánticamente
        hits.sort_by_key(|el| el.region.area);
        Some(SelectionTarget::Element(hits[0].id))
    }
}

// ─── Helpers internos ─────────────────────────────────────────────────────────

/// Score de snap para un elemento dado un cursor (cx, cy).
/// Menor score = mejor candidato.
#[inline]
fn snap_score(el: &UIElement, cx: f32, cy: f32) -> f32 {
    let b = &el.region.bbox;
    let ecx = (b.xmin + b.xmax) as f32 / 2.0;
    let ecy = (b.ymin + b.ymax) as f32 / 2.0;
    let dx = ecx - cx;
    let dy = ecy - cy;
    let distance = (dx * dx + dy * dy).sqrt();
    // ε = 0.0001 para evitar división por cero cuando importance_score = 0
    distance / (el.importance_score + 0.0001)
}

// ─── Tests unitarios ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_action_mapping_completo() {
        let engine = InteractionEngine::new();
        assert_eq!(engine.infer_action(UIElementType::Button),  UIAction::Click);
        assert_eq!(engine.infer_action(UIElementType::Text),    UIAction::SelectText);
        assert_eq!(engine.infer_action(UIElementType::Input),   UIAction::FocusInput);
        assert_eq!(engine.infer_action(UIElementType::Panel),   UIAction::ExpandPanel);
        assert_eq!(engine.infer_action(UIElementType::Image),   UIAction::ViewImage);
        assert_eq!(engine.infer_action(UIElementType::Unknown), UIAction::NoAction);
    }
}
