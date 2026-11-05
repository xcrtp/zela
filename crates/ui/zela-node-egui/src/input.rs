//! Translates egui pointer/keyboard input into editor `Command`s.
//!
//! State machine:
//! - press: socket → connection drag; resize grip → card resize;
//!   card header → select + node drag (if `interaction.draggable`);
//!   middle/right button → canvas pan; empty area → clear selection
//! - motion: MoveNode / ResizeNode / UpdateConnectionDrag / canvas pan
//! - release: finish or cancel the pending connection
//!
//! Which interactions a node accepts comes from its
//! [`ze_node_core::NodeMeta::interaction`] metadata (non-draggable cards ignore
//! header drags, non-resizable cards expose no grip); the hit policy itself
//! is pluggable via `Editor::set_hit_policy`.

use egui::Vec2;
use ze_node_core::{EdgeId, NodeId};
use zela_node_editor::NodePos;
use zela_node_editor::{Command, Editor, EditorError, HitTarget};

use crate::canvas::GraphCanvas;

/// Applies a command and reports rejection instead of silently dropping it.
///
/// A failed UI command (delete / connect / move) used to be invisible: the
/// user clicked and nothing happened, with no diagnostic. `tracing` keeps the
/// hot path allocation-free when no subscriber is installed.
fn apply(editor: &mut Editor, cmd: Command) -> bool {
    match editor.apply(cmd) {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!("node editor command rejected: {err}");
            false
        }
    }
}

/// Reports a failed undo/redo step (empty history is `Ok`, so this is a real error).
fn report_history(result: Result<(), EditorError>) {
    if let Err(err) = result {
        tracing::warn!("node editor history step failed: {err}");
    }
}

#[derive(Debug, Clone, Copy)]
enum DragState {
    /// Dragging a card by its header.
    Node { node: NodeId, grab_offset: NodePos },
    /// Dragging the bottom-right resize grip; `anchor` is the card's top-left
    /// corner (fixed for the whole gesture).
    Resize { node: NodeId, anchor: NodePos },
    /// Dragging a connection out of an output socket.
    Connection,
    /// Panning the canvas with the middle / right mouse button.
    Pan,
}

/// Owns the per-frame pointer interaction state.
#[derive(Default)]
pub struct InputHandler {
    drag: Option<DragState>,
    /// Right-click on empty canvas: graph position the host may turn into a
    /// spawn menu (consumed via [`Self::take_context_menu_pos`]).
    context_menu_at: Option<NodePos>,
}

impl InputHandler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pops a pending right-click-on-empty-canvas request.
    pub fn take_context_menu_pos(&mut self) -> Option<NodePos> {
        self.context_menu_at.take()
    }

    /// Read this frame's input and apply the resulting commands.
    pub fn handle(&mut self, ui: &egui::Ui, canvas: &mut GraphCanvas, editor: &mut Editor) {
        self.handle_keys(ui, editor);
        self.handle_pointer(ui, canvas, editor);
    }

    fn handle_keys(&self, ui: &egui::Ui, editor: &mut Editor) {
        let ctrl = ui.input(|i| i.modifiers.ctrl);
        let shift = ui.input(|i| i.modifiers.shift);

        // Don't delete nodes while the user is typing in a text field.
        let text_focused = ui.memory(|m| m.focused().is_some());
        if !text_focused
            && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        {
            let selected: Vec<NodeId> = editor.selection().iter().copied().collect();
            for id in selected {
                apply(editor, Command::RemoveNode(id));
            }
        }

        if ctrl && !shift && ui.input(|i| i.key_pressed(egui::Key::Z)) {
            report_history(editor.undo());
        }
        if ctrl
            && (ui.input(|i| i.key_pressed(egui::Key::Y))
                || shift && ui.input(|i| i.key_pressed(egui::Key::Z)))
        {
            report_history(editor.redo());
        }
    }

    fn handle_pointer(&mut self, ui: &egui::Ui, canvas: &mut GraphCanvas, editor: &mut Editor) {
        let Some(pos) = ui.input(|i| i.pointer.latest_pos()) else {
            return;
        };
        let graph_pos = canvas.to_graph(pos);

        // 每帧把指针位置交给编辑器的路由（hover 在编辑器里，UI 不重复保存）。
        let hit = if canvas.viewport().contains(pos) {
            editor.pointer_moved(graph_pos)
        } else {
            editor.pointer_left();
            HitTarget::Nothing
        };

        let primary_pressed = ui.input(|i| i.pointer.primary_pressed());
        let pan_pressed = ui.input(|i| {
            i.pointer.secondary_pressed() || i.pointer.button_pressed(egui::PointerButton::Middle)
        });

        // Pan gestures can start anywhere, even outside the viewport.
        if pan_pressed {
            self.drag = Some(DragState::Pan);
            // Right-click on empty canvas requests the spawn menu.
            if ui.input(|i| i.pointer.secondary_pressed())
                && canvas.viewport().contains(pos)
                && matches!(hit, HitTarget::Nothing)
            {
                self.context_menu_at = Some(graph_pos);
            }
        }

        // Left-button presses only start interactions inside the canvas.
        if primary_pressed && canvas.viewport().contains(pos) {
            // 按下走编辑器的路由：建立 focus + capture（拖拽期间不跟命中走）。
            match editor.pointer_pressed(graph_pos) {
                HitTarget::OutputSocket(id) => {
                    if apply(editor, Command::BeginConnection(id)) {
                        self.drag = Some(DragState::Connection);
                    }
                }
                HitTarget::InputSocket(id) => {
                    // Grab the existing incoming edge and re-drag it.
                    if let Some(source) = editor.graph().upstream(id).pop() {
                        let edge = EdgeId { source, target: id };
                        apply(editor, Command::Disconnect(edge));
                        if apply(editor, Command::BeginConnection(source)) {
                            self.drag = Some(DragState::Connection);
                        }
                    }
                }
                HitTarget::ResizeHandle(id) => {
                    // Grab the resize grip: select the card and start a resize
                    // gesture. The grip only exists for interaction-resizable
                    // nodes (the hit policy already filtered them), this is
                    // the defense-in-depth check.
                    let resizable = editor
                        .graph()
                        .get_node(id)
                        .is_some_and(|n| editor.presentation_of(n).interaction.resizable);
                    if resizable && let Some(rect) = editor.node_rect(id) {
                        let additive = ui.input(|i| i.modifiers.shift);
                        apply(editor, Command::Select { node: id, additive });
                        self.drag = Some(DragState::Resize {
                            node: id,
                            anchor: rect.min,
                        });
                    }
                }
                HitTarget::Node(id) => {
                    let interaction = editor
                        .graph()
                        .get_node(id)
                        .map(|n| editor.presentation_of(n).interaction);
                    let selectable = interaction.as_ref().is_none_or(|i| i.selectable);
                    let draggable = interaction.as_ref().is_none_or(|i| i.draggable);
                    if selectable {
                        let additive = ui.input(|i| i.modifiers.shift);
                        apply(editor, Command::Select { node: id, additive });
                    }
                    // Only the header band of a draggable card starts a move;
                    // body clicks fall through to the card's body widgets.
                    let header_height = editor.socket_layout().header_height();
                    if draggable
                        && let Some(rect) = editor.node_rect(id)
                        && graph_pos.y <= rect.min.y + header_height
                    {
                        self.drag = Some(DragState::Node {
                            node: id,
                            grab_offset: graph_pos - rect.min,
                        });
                    }
                }
                HitTarget::Edge(_) => {
                    // Edge click-to-delete is deferred to stage 10.
                }
                HitTarget::Nothing => {
                    apply(editor, Command::ClearSelection);
                }
            }
        }

        // --- hover cursor feedback (no button pressed) ---
        if !ui.input(|i| i.pointer.any_down())
            && canvas.viewport().contains(pos)
            && matches!(hit, HitTarget::ResizeHandle(_))
        {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeNwSe);
        }

        // --- active drag motion ---
        if ui.input(|i| i.pointer.any_down()) {
            match self.drag {
                Some(DragState::Node { node, grab_offset }) => {
                    apply(
                        editor,
                        Command::MoveNode {
                            node,
                            to: graph_pos - grab_offset,
                        },
                    );
                }
                Some(DragState::Resize { node, anchor }) => {
                    // Card's top-left stays anchored; size tracks the pointer.
                    // The editor clamps to the node's min/max bounds.
                    apply(
                        editor,
                        Command::ResizeNode {
                            node,
                            size: graph_pos - anchor,
                        },
                    );
                }
                Some(DragState::Connection) => {
                    apply(editor, Command::UpdateConnectionDrag(graph_pos));
                }
                Some(DragState::Pan) => {
                    let delta = ui.input(|i| i.pointer.delta());
                    canvas.pan += Vec2::new(delta.x, delta.y);
                }
                None => {}
            }
        }

        // --- release ---
        let released = ui.input(|i| {
            i.pointer.primary_released()
                || i.pointer.secondary_released()
                || i.pointer.button_released(egui::PointerButton::Middle)
        });
        if released {
            // 抬起：解除 capture（focus 保留到下一次按下）。
            editor.pointer_released();
            if matches!(self.drag, Some(DragState::Connection)) && canvas.viewport().contains(pos) {
                // Prefer the magnetic snap target (the pin the wire is locked
                // onto); fall back to the direct hit for a precise drop.
                let target = editor
                    .pending()
                    .and_then(|p| p.hover_target)
                    .or_else(|| match hit {
                        HitTarget::InputSocket(id) => Some(id),
                        _ => None,
                    });
                match target {
                    Some(target) => {
                        apply(editor, Command::CompleteConnection(target));
                    }
                    None => {
                        apply(editor, Command::CancelConnection);
                    }
                }
            }
            self.drag = None;
        }
    }
}
