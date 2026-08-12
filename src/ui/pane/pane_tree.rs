use gpui::prelude::*;
use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};

use super::editor_group::{EditorGroup, EditorGroupEvent};
use super::types::{DropPlacement, SplitDirection, TabContent, TabDrag, TabItem};
use crate::core::editor::Editor;

#[derive(Clone)]
pub enum PaneNode {
    Leaf { id: usize, group: Entity<EditorGroup> },
    HSplit { id: usize, left: Box<PaneNode>, right: Box<PaneNode> },
    VSplit { id: usize, top: Box<PaneNode>, bottom: Box<PaneNode> },
}

pub enum PaneTreeEvent {
    ActiveEditorChanged,
}

#[allow(dead_code)]
pub struct PaneTree {
    root: Option<PaneNode>,
    active_group_id: Option<usize>,
    next_group_id: usize,
    next_split_id: usize,
    next_tab_id: usize,
}

#[allow(dead_code)]
impl PaneTree {
    pub fn new() -> Self {
        Self {
            root: None,
            active_group_id: None,
            next_group_id: 1,
            next_split_id: 1,
            next_tab_id: 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub fn active_group_id(&self) -> Option<usize> {
        self.active_group_id
    }

    pub fn active_group(&self, _cx: &App) -> Option<Entity<EditorGroup>> {
        let id = self.active_group_id?;
        self.find_group(id)
    }

    pub fn active_editor(&self, cx: &App) -> Option<Entity<Editor>> {
        self.active_group(cx)?.read(cx).active_editor(cx)
    }

    pub fn find_group(&self, group_id: usize) -> Option<Entity<EditorGroup>> {
        fn search(node: &PaneNode, target_id: usize) -> Option<Entity<EditorGroup>> {
            match node {
                PaneNode::Leaf { id, group } => {
                    if *id == target_id {
                        Some(group.clone())
                    } else {
                        None
                    }
                }
                PaneNode::HSplit { left, right, .. } => search(left, target_id).or_else(|| search(right, target_id)),
                PaneNode::VSplit { top, bottom, .. } => search(top, target_id).or_else(|| search(bottom, target_id)),
            }
        }
        self.root.as_ref().and_then(|r| search(r, group_id))
    }

    pub fn all_groups(&self) -> Vec<Entity<EditorGroup>> {
        fn collect(node: &PaneNode, acc: &mut Vec<Entity<EditorGroup>>) {
            match node {
                PaneNode::Leaf { group, .. } => acc.push(group.clone()),
                PaneNode::HSplit { left, right, .. } => {
                    collect(left, acc);
                    collect(right, acc);
                }
                PaneNode::VSplit { top, bottom, .. } => {
                    collect(top, acc);
                    collect(bottom, acc);
                }
            }
        }
        let mut groups = Vec::new();
        if let Some(root) = &self.root {
            collect(root, &mut groups);
        }
        groups
    }

    fn create_group(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<EditorGroup> {
        let id = self.next_group_id;
        self.next_group_id += 1;

        let group = cx.new(|cx| EditorGroup::new(id, cx));

        cx.subscribe_in(&group, window, move |this, _group, event: &EditorGroupEvent, window, cx| match event {
            EditorGroupEvent::Focused => {
                this.set_active_group(id, cx);
            }
            EditorGroupEvent::TabChanged => {
                this.sync_active_states(cx);
                cx.emit(PaneTreeEvent::ActiveEditorChanged);
                cx.notify();
            }
            EditorGroupEvent::Split { direction, new_content } => {
                this.split_group(id, *direction, new_content.clone(), window, cx);
            }
            EditorGroupEvent::CloseTab(_) => {
                this.sync_active_states(cx);
                cx.emit(PaneTreeEvent::ActiveEditorChanged);
                cx.notify();
            }
            EditorGroupEvent::CloseGroup => {
                this.remove_group(id, window, cx);
            }
            EditorGroupEvent::DropTab { drag, target_index } => {
                this.move_tab(*drag, id, *target_index, window, cx);
            }
            EditorGroupEvent::SplitWithDrop { drag, placement } => {
                this.split_with_drop(*drag, id, *placement, window, cx);
            }
        })
        .detach();

        group
    }

    pub fn open_tab(&mut self, content: TabContent, window: &mut Window, cx: &mut Context<Self>) -> usize {
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;

        let tab = TabItem::new(tab_id, content);

        let target_group = if let Some(active_id) = self.active_group_id
            && let Some(g) = self.find_group(active_id)
        {
            g
        } else if let Some(first_g) = self.all_groups().first().cloned() {
            first_g
        } else {
            let new_group = self.create_group(window, cx);
            let group_id = new_group.read(cx).id;
            self.root = Some(PaneNode::Leaf {
                id: group_id,
                group: new_group.clone(),
            });
            self.active_group_id = Some(group_id);
            new_group
        };

        let target_group_id = target_group.read(cx).id;
        self.active_group_id = Some(target_group_id);

        target_group.update(cx, |g, cx| {
            g.add_tab(tab, true, window, cx);
        });

        self.sync_active_states(cx);
        cx.emit(PaneTreeEvent::ActiveEditorChanged);
        cx.notify();

        tab_id
    }

    pub fn split_group(&mut self, target_group_id: usize, direction: SplitDirection, content: TabContent, window: &mut Window, cx: &mut Context<Self>) {
        let new_group = self.create_group(window, cx);
        let new_group_id = new_group.read(cx).id;

        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = TabItem::new(tab_id, content);

        new_group.update(cx, |g, cx| {
            g.add_tab(tab, true, window, cx);
        });

        let split_id = self.next_split_id;
        self.next_split_id += 1;

        let new_leaf = PaneNode::Leaf {
            id: new_group_id,
            group: new_group.clone(),
        };

        if let Some(root) = self.root.take() {
            self.root = Some(replace_node(root, target_group_id, &mut |target_group| match direction {
                SplitDirection::Horizontal => PaneNode::HSplit {
                    id: split_id,
                    left: Box::new(PaneNode::Leaf {
                        id: target_group_id,
                        group: target_group,
                    }),
                    right: Box::new(new_leaf.clone()),
                },
                SplitDirection::Vertical => PaneNode::VSplit {
                    id: split_id,
                    top: Box::new(PaneNode::Leaf {
                        id: target_group_id,
                        group: target_group,
                    }),
                    bottom: Box::new(new_leaf.clone()),
                },
            }));
        }

        self.active_group_id = Some(new_group_id);
        self.sync_active_states(cx);
        cx.emit(PaneTreeEvent::ActiveEditorChanged);
        cx.notify();
    }

    pub fn split_with_drop(&mut self, drag: TabDrag, target_group_id: usize, placement: DropPlacement, window: &mut Window, cx: &mut Context<Self>) {
        let from_group = self.find_group(drag.from_group_id);
        let Some(from_group) = from_group else {
            return;
        };

        let removed_tab = from_group.update(cx, |g, cx| g.remove_tab_by_id(drag.tab_id, window, cx));
        let Some(tab) = removed_tab else {
            return;
        };

        // Check if from_group is now empty
        let from_group_empty = from_group.read(cx).is_empty();

        match placement {
            DropPlacement::Center => {
                if let Some(target_group) = self.find_group(target_group_id) {
                    target_group.update(cx, |g, cx| {
                        g.add_tab(tab, true, window, cx);
                    });
                }
            }
            DropPlacement::Left | DropPlacement::Right | DropPlacement::Top | DropPlacement::Bottom => {
                let new_group = self.create_group(window, cx);
                let new_group_id = new_group.read(cx).id;
                new_group.update(cx, |g, cx| {
                    g.add_tab(tab, true, window, cx);
                });

                let split_id = self.next_split_id;
                self.next_split_id += 1;

                let new_leaf = PaneNode::Leaf {
                    id: new_group_id,
                    group: new_group.clone(),
                };

                if let Some(root) = self.root.take() {
                    self.root = Some(replace_node(root, target_group_id, &mut |target_group| {
                        let target_leaf = PaneNode::Leaf {
                            id: target_group_id,
                            group: target_group,
                        };
                        match placement {
                            DropPlacement::Left => PaneNode::HSplit {
                                id: split_id,
                                left: Box::new(new_leaf.clone()),
                                right: Box::new(target_leaf),
                            },
                            DropPlacement::Right => PaneNode::HSplit {
                                id: split_id,
                                left: Box::new(target_leaf),
                                right: Box::new(new_leaf.clone()),
                            },
                            DropPlacement::Top => PaneNode::VSplit {
                                id: split_id,
                                top: Box::new(new_leaf.clone()),
                                bottom: Box::new(target_leaf),
                            },
                            DropPlacement::Bottom => PaneNode::VSplit {
                                id: split_id,
                                top: Box::new(target_leaf),
                                bottom: Box::new(new_leaf.clone()),
                            },
                            DropPlacement::Center => target_leaf,
                        }
                    }));
                }
                self.active_group_id = Some(new_group_id);
            }
        }

        if from_group_empty {
            self.remove_group(drag.from_group_id, window, cx);
        }

        self.sync_active_states(cx);
        cx.emit(PaneTreeEvent::ActiveEditorChanged);
        cx.notify();
    }

    pub fn move_tab(&mut self, drag: TabDrag, to_group_id: usize, target_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let from_group = self.find_group(drag.from_group_id);
        let to_group = self.find_group(to_group_id);

        if let (Some(from_g), Some(to_g)) = (from_group, to_group) {
            let removed_tab = from_g.update(cx, |g, cx| g.remove_tab_by_id(drag.tab_id, window, cx));

            if let Some(tab) = removed_tab {
                to_g.update(cx, |g, cx| {
                    g.insert_tab(target_index, tab, true, window, cx);
                });
                self.active_group_id = Some(to_group_id);

                if from_g.read(cx).is_empty() {
                    self.remove_group(drag.from_group_id, window, cx);
                }
            }
        }

        self.sync_active_states(cx);
        cx.emit(PaneTreeEvent::ActiveEditorChanged);
        cx.notify();
    }

    pub fn remove_group(&mut self, group_id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(root) = self.root.take() {
            self.root = remove_node(root, group_id);
        }

        if self.active_group_id == Some(group_id) {
            self.active_group_id = self.all_groups().first().map(|g| g.read(cx).id);
            if let Some(group) = self.active_group(cx)
                && let Some(tab) = group.read(cx).active_tab()
            {
                tab.focus_handle(cx).focus(window);
            }
        }

        self.sync_active_states(cx);
        cx.emit(PaneTreeEvent::ActiveEditorChanged);
        cx.notify();
    }

    pub fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.active_group(cx) {
            let tab_id = group.read(cx).active_tab().map(|t| t.id);
            if let Some(tab_id) = tab_id {
                group.update(cx, |g, cx| {
                    g.close_tab(tab_id, window, cx);
                });
            }
        }
    }

    pub fn set_active_group(&mut self, group_id: usize, cx: &mut Context<Self>) {
        self.active_group_id = Some(group_id);
        self.sync_active_states(cx);
        cx.emit(PaneTreeEvent::ActiveEditorChanged);
        cx.notify();
    }

    fn sync_active_states(&self, cx: &mut Context<Self>) {
        let active_id = self.active_group_id;
        for group in self.all_groups() {
            let is_active = Some(group.read(cx).id) == active_id;
            group.update(cx, |g, cx| {
                if g.is_active_group != is_active {
                    g.is_active_group = is_active;
                    cx.notify();
                }
            });
        }
    }
}

fn remove_node(node: PaneNode, target_id: usize) -> Option<PaneNode> {
    match node {
        PaneNode::Leaf { id, .. } => {
            if id == target_id {
                None
            } else {
                Some(node)
            }
        }
        PaneNode::HSplit { id, left, right } => {
            let left_opt = remove_node(*left, target_id);
            let right_opt = remove_node(*right, target_id);
            match (left_opt, right_opt) {
                (Some(l), Some(r)) => Some(PaneNode::HSplit {
                    id,
                    left: Box::new(l),
                    right: Box::new(r),
                }),
                (Some(l), None) => Some(l),
                (None, Some(r)) => Some(r),
                (None, None) => None,
            }
        }
        PaneNode::VSplit { id, top, bottom } => {
            let top_opt = remove_node(*top, target_id);
            let bottom_opt = remove_node(*bottom, target_id);
            match (top_opt, bottom_opt) {
                (Some(t), Some(b)) => Some(PaneNode::VSplit {
                    id,
                    top: Box::new(t),
                    bottom: Box::new(b),
                }),
                (Some(t), None) => Some(t),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            }
        }
    }
}

fn replace_node<F>(node: PaneNode, target_id: usize, f: &mut F) -> PaneNode
where
    F: FnMut(Entity<EditorGroup>) -> PaneNode,
{
    match node {
        PaneNode::Leaf { id, group } => {
            if id == target_id {
                f(group)
            } else {
                PaneNode::Leaf { id, group }
            }
        }
        PaneNode::HSplit { id, left, right } => PaneNode::HSplit {
            id,
            left: Box::new(replace_node(*left, target_id, f)),
            right: Box::new(replace_node(*right, target_id, f)),
        },
        PaneNode::VSplit { id, top, bottom } => PaneNode::VSplit {
            id,
            top: Box::new(replace_node(*top, target_id, f)),
            bottom: Box::new(replace_node(*bottom, target_id, f)),
        },
    }
}

fn render_node(node: &PaneNode) -> AnyElement {
    let content = match node {
        PaneNode::Leaf { group, .. } => group.clone().into_any_element(),
        PaneNode::HSplit { id, left, right } => h_resizable(ElementId::NamedInteger("h-split".into(), *id as u64))
            .child(resizable_panel().child(render_node(left)))
            .child(resizable_panel().child(render_node(right)))
            .into_any_element(),
        PaneNode::VSplit { id, top, bottom } => v_resizable(ElementId::NamedInteger("v-split".into(), *id as u64))
            .child(resizable_panel().child(render_node(top)))
            .child(resizable_panel().child(render_node(bottom)))
            .into_any_element(),
    };

    div().size_full().min_w_0().min_h_0().overflow_hidden().child(content).into_any_element()
}

impl EventEmitter<PaneTreeEvent> for PaneTree {}

impl Render for PaneTree {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(root) = &self.root {
            div()
                .id("pane-tree-root")
                .size_full()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .child(render_node(root))
        } else {
            div()
                .id("pane-tree-empty")
                .size_full()
                .flex()
                .justify_center()
                .items_center()
                .bg(cx.theme().background)
                .child(div().text_xl().text_color(cx.theme().muted_foreground).child("Nothing is open"))
        }
    }
}
