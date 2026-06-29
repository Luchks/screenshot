// ─── Etapa 3: Hierarchy Builder ───────────────────────────────────────────────
//
// Construye un árbol de contención sobre Vec<UIElement>.
// Algoritmo: O(n²) containment scoring + best-parent selection.
// No modifica Etapas 1 ni 2.

use crate::{Rect, Region};
use crate::classify::{ClassificationResult, UIElementType};
use crate::features::RegionFeatures;

// ── Tipos públicos de Etapa 3 ─────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct UIElement {
    pub id:              usize,
    pub region:          Region,
    pub features:        RegionFeatures,
    pub element_type:    UIElementType,
    pub confidence:      f32,
    pub importance_score: f32,
    pub children:        Vec<usize>,
    pub parent:          Option<usize>,
}

/// Árbol jerárquico de la UI: índices raíz + acceso por id.
#[derive(Clone, Debug)]
pub struct UIHierarchyTree {
    /// IDs de elementos sin padre (nivel 0)
    pub roots: Vec<usize>,
    /// Todos los elementos, indexados por su campo `id`
    pub elements: Vec<UIElement>,
}

impl UIHierarchyTree {
    /// Devuelve los hijos directos de un elemento dado su id.
    pub fn children_of(&self, id: usize) -> &[usize] {
        &self.elements[id].children
    }

    /// DFS desde todas las raíces, en orden de visita.
    pub fn depth_first(&self) -> Vec<usize> {
        let mut result = Vec::with_capacity(self.elements.len());
        let mut stack = self.roots.clone();
        // Para preservar orden visual de izquierda a derecha / arriba a abajo,
        // revertimos las raíces ya que es un LIFO stack.
        stack.reverse();

        while let Some(id) = stack.pop() {
            result.push(id);
            // Revertir hijos para mantener orden correcto al sacarlos del stack
            let mut children = self.elements[id].children.clone();
            children.reverse();
            stack.extend(children);
        }
        result
    }
}

// ── Lógica de Contención (Heurística Estricta) ────────────────────────────────

/// Evalúa qué tan probable es que `child` esté contenido dentro de `parent`.
/// Devuelve un score en [0, 1]. Si es 0, no hay contención viable.
fn containment_score(parent: &Region, child: &Region) -> f32 {
    let p = &parent.bbox;
    let c = &child.bbox;

    // Condición necesaria: intersección casi total o total.
    // Tolerancia de 2 píxeles por desajustes de segmentación morfológica.
    let margin = 2;
    let is_contained = c.xmin + margin >= p.xmin 
        && c.ymin + margin >= p.ymin
        && c.xmax <= p.xmax + margin
        && c.ymax <= p.ymax + margin;

    if !is_contained {
        return 0.0;
    }

    // Si el padre es idéntico en tamaño, no es un contenedor genuino (mismo bloque visual)
    if p.xmin == c.xmin && p.ymin == c.ymin && p.xmax == c.xmax && p.ymax == c.ymax {
        return 0.0;
    }

    // Regla de Oro: El contenedor debe ser mayor en área.
    if parent.area <= child.area {
        return 0.0;
    }

    // Scoring proporcional: preferimos el contenedor más pequeño y ajustado (envoltura convexa mínima)
    let area_ratio = child.area as f32 / parent.area as f32;
    
    // Penalización si el contenedor gigante se traga algo minúsculo sin relación directa estructural 
    // (ej: la raíz de la pantalla no es el padre inmediato preferido si hay un panel intermedio)
    0.3 + 0.7 * area_ratio
}

// ── Pipeline Principal ────────────────────────────────────────────────────────

/// Orquesta la extracción, clasificación y estructuración jerárquica de las regiones.
pub fn build_ui_semantic_layer(regions: &[Region], screen_w: usize, screen_h: usize) -> (Vec<UIElement>, UIHierarchyTree) {
    use crate::features::RegionFeatures;
    use crate::classify::classify_region;

    // ── Paso 1: Inicializar Elementos con su clasificación individual ───────
    let mut elements: Vec<UIElement> = regions
        .iter()
        .enumerate()
        .map(|(i, reg)| {
            let f = RegionFeatures::new(reg, screen_w, screen_h);
            let res = classify_region(&f);
            UIElement {
                id: i,
                region: reg.clone(),
                features: f,
                element_type: res.element_type,
                confidence: res.confidence,
                importance_score: res.importance_score,
                children: Vec::new(),
                parent: None,
            }
        })
        .collect();

    let n = elements.len();

    // ── Paso 2: Matriz de Contención e Inferencia de Enlaces ───────────────
    // Buscamos para cada elemento cuál es su mejor contenedor padre.
    for i in 0..n {
        let mut best_parent: Option<usize> = None;
        let mut max_score = 0.0;

        for j in 0..n {
            if i == j { continue; }
            let score = containment_score(&elements[j].region, &elements[i].region);
            if score > max_score {
                max_score = score;
                best_parent = Some(j);
            }
        }

        if max_score > 0.0 {
            elements[i].parent = best_parent;
        }
    }

    // ── Paso 3: Rellenar vectores de hijos basados en la relación paternal ──
    // Hacemos una pasada limpia recolectando relaciones inversas.
    let mut parent_child_pairs = Vec::new();
    for el in &elements {
        if let Some(p_id) = el.parent {
            parent_child_pairs.push((p_id, el.id));
        }
    }
    for (p_id, c_id) in parent_child_pairs {
        elements[p_id].children.push(c_id);
    }

    // ── Paso 4: Ordenar los hijos por importancia (Solución al Préstamo Mutable) ──
    // Se recopilan los puntajes primero para evitar colisionar con el préstamo de `elements`
    let importance_scores: Vec<f32> = elements.iter().map(|el| el.importance_score).collect();

    for el in &mut elements {
        el.children.sort_by(|&a, &b| {
            importance_scores[b]
                .partial_cmp(&importance_scores[a])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // ── Paso 5: Construir el Árbol Final ────────────────────────────────────
    let roots: Vec<usize> = elements.iter()
        .filter(|el| el.parent.is_none())
        .map(|el| el.id)
        .collect();

    let tree = UIHierarchyTree {
        roots,
        elements: elements.clone(),
    };

    (elements, tree)
}

// ── Debug helper ──────────────────────────────────────────────────────────────

/// Imprime el árbol en stdout con indentación para depuración.
pub fn print_ui_tree(tree: &UIHierarchyTree) {
    fn print_node(tree: &UIHierarchyTree, id: usize, depth: usize) {
        let el = &tree.elements[id];
        let indent = "  ".repeat(depth);
        println!(
            "{}[{}] {:?} | score={:.2} conf={:.2} | bbox=({},{})→({},{}) area={}",
            indent,
            el.id,
            el.element_type.label(),
            el.importance_score,
            el.confidence,
            el.region.bbox.xmin,
            el.region.bbox.ymin,
            el.region.bbox.xmax,
            el.region.bbox.ymax,
            el.region.area,
        );
        for &child_id in &el.children {
            print_node(tree, child_id, depth + 1);
        }
    }

    println!("\n─── Árbol de Jerarquía Semántica UI ───");
    for &root_id in &tree.roots {
        print_node(tree, root_id, 0);
    }
    println!("────────────────────────────────────────\n");
}