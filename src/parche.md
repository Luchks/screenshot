Tengo todo lo que necesito. `blocks` es `Vec<Rect>` y `Rect` implementa `Copy` (línea 9), así que `&blocks[i]` funciona perfectamente.

---

## Bloque modificado

**Ubicación exacta:** reemplaza el brazo `SelectionMode::HintMode` del `match sel.mode`, líneas **290–318**.

```rust
SelectionMode::HintMode => {
    if window.is_key_down(Key::F) && !f_key_pressed {
        f_key_pressed = true;
        active_candidates.clear();
        active_hints.clear();
        let (r, lbl) = snap_rect(cx, cy);
        sel = Selection { rect: r, mode: SelectionMode::AutoSnap, source_label: lbl };
        acted = true;
    }

    if hint_key_pressed.is_none() {
        for &hk in HINT_KEYS {
            if window.is_key_down(hk) {
                hint_key_pressed = Some(hk);
                if let Some(candidate_index) = find_hint_by_key(&active_hints, hk) {
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
                acted = true;
                break;
            }
        }
    }
}
```

---

## Cambios explicados

**1. Cálculo del centro y actualización de `cx`/`cy`**
```rust
cx = (rect.xmin + rect.xmax) / 2;
cy = (rect.ymin + rect.ymax) / 2;
```
Se calcula el centro del `Rect` del candidato y se actualiza el cursor lógico. No se toca el ratón físico.

**2. Referencia en lugar de copia**
```rust
let rect = &blocks[candidate.id.0];
```
Se toma `&Rect` en vez de mover/copiar. Aunque `Rect` implementa `Copy`, usar la referencia es más explícito y más correcto semánticamente según tu instrucción.

**3. `snap_rect` con las nuevas coordenadas**
```rust
let (snapped, lbl) = snap_rect(cx, cy);
```
Se ejecuta después de actualizar `cx`/`cy`, por lo que opera ya sobre el centro del candidato elegido.

**4. Reconstrucción de `sel` antes de limpiar**
```rust
sel = Selection { rect: snapped, mode: SelectionMode::AutoSnap, source_label: lbl };
active_candidates.clear();
active_hints.clear();
```
Primero se actualiza la selección completa, luego se limpia el estado temporal de HintMode. El orden es el que pediste.

**5. `acted = true` siempre que se detecta una tecla hint**
Se mueve fuera del `if let Some(...)` para que `last_input_time` se actualice incluso si la tecla no corresponde a ningún hint registrado (evita repeticiones indeseadas del mismo evento).
