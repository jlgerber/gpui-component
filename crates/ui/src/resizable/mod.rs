use std::ops::Range;

use gpui::{
    Along, App, Axis, Bounds, Context, ElementId, EventEmitter, IsZero, Pixels, Window, px,
};

mod panel;
mod resize_handle;
pub use panel::*;
pub(crate) use resize_handle::*;

pub(crate) const PANEL_MIN_SIZE: Pixels = px(100.);

/// Create a [`ResizablePanelGroup`] with horizontal resizing
pub fn h_resizable(id: impl Into<ElementId>) -> ResizablePanelGroup {
    ResizablePanelGroup::new(id).axis(Axis::Horizontal)
}

/// Create a [`ResizablePanelGroup`] with vertical resizing
pub fn v_resizable(id: impl Into<ElementId>) -> ResizablePanelGroup {
    ResizablePanelGroup::new(id).axis(Axis::Vertical)
}

/// Create a [`ResizablePanel`].
pub fn resizable_panel() -> ResizablePanel {
    ResizablePanel::new()
}

/// State for a [`ResizablePanel`]
#[derive(Debug, Clone)]
pub struct ResizableState {
    /// The `axis` will sync to actual axis of the ResizablePanelGroup in use.
    axis: Axis,
    panels: Vec<ResizablePanelState>,
    sizes: Vec<Pixels>,
    pub(crate) resizing_panel_ix: Option<usize>,
    bounds: Bounds<Pixels>,
}

impl Default for ResizableState {
    fn default() -> Self {
        Self {
            axis: Axis::Horizontal,
            panels: vec![],
            sizes: vec![],
            resizing_panel_ix: None,
            bounds: Bounds::default(),
        }
    }
}

impl ResizableState {
    /// Get the size of the panels.
    pub fn sizes(&self) -> &Vec<Pixels> {
        &self.sizes
    }

    /// Programmatically resize the panel at `ix` to `size`, redistributing
    /// space among siblings using the same logic as a drag.
    ///
    /// Sizes are clamped to the panel's `size_range` and to the container.
    /// Emits `ResizablePanelEvent::Resized` so subscribers (e.g. preference
    /// persistence) see the change just as if the user had dragged a handle.
    ///
    /// Out-of-range indices are a no-op. For the last panel, space is taken
    /// from the previous sibling (the last panel has no handle of its own).
    pub fn resize_panel(
        &mut self,
        ix: usize,
        size: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ix >= self.sizes.len() {
            return;
        }
        if ix + 1 < self.sizes.len() {
            self.resize_panel_at_handle(ix, size, window, cx);
        } else if ix > 0 {
            // Last panel: drive its size by resizing the previous sibling so
            // the freed space lands here.
            let delta = self.sizes[ix] - size;
            let prev = self.sizes[ix - 1];
            self.resize_panel_at_handle(ix - 1, prev + delta, window, cx);
        }
        self.done_resizing(cx);
    }

    pub(crate) fn insert_panel(
        &mut self,
        size: Option<Pixels>,
        ix: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let panel_state = ResizablePanelState {
            size,
            ..Default::default()
        };

        let size = size.unwrap_or(PANEL_MIN_SIZE);

        // We make sure that the size always sums up to the container size
        // by reducing the size of all other panels first.
        let container_size = self.container_size().max(px(1.));
        let total_leftover_size = (container_size - size).max(px(1.));

        for (i, panel) in self.panels.iter_mut().enumerate() {
            let ratio = self.sizes[i] / container_size;
            self.sizes[i] = total_leftover_size * ratio;
            panel.size = Some(self.sizes[i]);
        }

        if let Some(ix) = ix {
            self.panels.insert(ix, panel_state);
            self.sizes.insert(ix, size);
        } else {
            self.panels.push(panel_state);
            self.sizes.push(size);
        };

        cx.notify();
    }

    pub(crate) fn sync_panels_count(
        &mut self,
        axis: Axis,
        panels_count: usize,
        cx: &mut Context<Self>,
    ) {
        let mut changed = self.axis != axis;
        self.axis = axis;

        if panels_count > self.panels.len() {
            let diff = panels_count - self.panels.len();
            self.panels
                .extend(vec![ResizablePanelState::default(); diff]);
            self.sizes.extend(vec![PANEL_MIN_SIZE; diff]);
            changed = true;
        }

        if panels_count < self.panels.len() {
            self.panels.truncate(panels_count);
            self.sizes.truncate(panels_count);
            changed = true;
        }

        if changed {
            // We need to make sure the total size is in line with the container size.
            self.adjust_to_container_size(cx);
        }
    }

    pub(crate) fn update_panel_size(
        &mut self,
        panel_ix: usize,
        bounds: Bounds<Pixels>,
        size_range: Range<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let size = bounds.size.along(self.axis);
        // This check is only necessary to stop the very first panel from resizing on its own
        // it needs to be passed when the panel is freshly created so we get the initial size,
        // but its also fine when it sometimes passes later.
        if self.sizes[panel_ix].as_f32() == PANEL_MIN_SIZE.as_f32() {
            self.sizes[panel_ix] = size;
            self.panels[panel_ix].size = Some(size);
        }
        self.panels[panel_ix].bounds = bounds;
        self.panels[panel_ix].size_range = size_range;
        cx.notify();
    }

    pub(crate) fn remove_panel(&mut self, panel_ix: usize, cx: &mut Context<Self>) {
        self.panels.remove(panel_ix);
        self.sizes.remove(panel_ix);
        if let Some(resizing_panel_ix) = self.resizing_panel_ix {
            if resizing_panel_ix > panel_ix {
                self.resizing_panel_ix = Some(resizing_panel_ix - 1);
            }
        }
        self.adjust_to_container_size(cx);
    }

    pub(crate) fn replace_panel(
        &mut self,
        panel_ix: usize,
        panel: ResizablePanelState,
        cx: &mut Context<Self>,
    ) {
        let old_size = self.sizes[panel_ix];

        self.panels[panel_ix] = panel;
        self.sizes[panel_ix] = old_size;
        self.adjust_to_container_size(cx);
    }

    pub(crate) fn clear(&mut self) {
        self.panels.clear();
        self.sizes.clear();
    }

    #[inline]
    pub(crate) fn container_size(&self) -> Pixels {
        self.bounds.size.along(self.axis)
    }

    pub(crate) fn done_resizing(&mut self, cx: &mut Context<Self>) {
        self.resizing_panel_ix = None;
        cx.emit(ResizablePanelEvent::Resized);
    }

    fn panel_size_range(&self, ix: usize) -> Range<Pixels> {
        let Some(panel) = self.panels.get(ix) else {
            return PANEL_MIN_SIZE..Pixels::MAX;
        };

        panel.size_range.clone()
    }

    fn sync_real_panel_sizes(&mut self, _: &App) {
        for (i, panel) in self.panels.iter().enumerate() {
            self.sizes[i] = panel.bounds.size.along(self.axis);
        }
    }

    /// Collapse panel `ix` to a thin strip of `strip` pixels, saving its
    /// current size for later restoration with [`Self::expand_panel`].
    ///
    /// The freed space is redistributed to siblings exactly as a drag would,
    /// but the strip size may go below [`PANEL_MIN_SIZE`] (the normal minimum
    /// is bypassed for the collapse target only).
    ///
    /// Out-of-range indices and already-collapsed panels are no-ops.
    pub fn collapse_panel(
        &mut self,
        ix: usize,
        strip: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ix >= self.sizes.len() {
            return;
        }
        if self.panels[ix].collapsed.is_some() {
            return;
        }
        // Save the current expanded size before modifying it.
        self.panels[ix].collapsed = Some(self.sizes[ix]);

        if ix + 1 < self.sizes.len() {
            // Non-last panel: resize it directly, allowing below-min.
            self.resize_panel_at_handle_inner(ix, strip, Some(ix), window, cx);
        } else if ix > 0 {
            // Last panel: drive via the previous handle, mirroring resize_panel.
            // The collapse_target tells the inner fn that panel `ix` (the last)
            // may shrink below its size_range minimum.
            let delta = self.sizes[ix] - strip;
            let prev = self.sizes[ix - 1];
            self.resize_panel_at_handle_inner(ix - 1, prev + delta, Some(ix), window, cx);
        }
        // (degenerate single-panel split: nothing to redistribute)
        self.done_resizing(cx);
    }

    /// Restore panel `ix` to its size before collapse.
    ///
    /// No-op if the panel is not collapsed or the index is out of range.
    /// Uses the normal [`Self::resize_panel`] path so siblings shrink back
    /// and the restored size is clamped to `size_range`.
    pub fn expand_panel(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.panels.get(ix) else {
            return;
        };
        let Some(saved) = panel.collapsed else {
            return;
        };
        // Clear before resize so is_collapsed() returns false immediately.
        self.panels[ix].collapsed = None;
        self.resize_panel(ix, saved, window, cx);
    }

    /// Returns `true` if panel `ix` is currently collapsed.
    ///
    /// Out-of-range indices return `false`.
    pub fn is_collapsed(&self, ix: usize) -> bool {
        self.panels
            .get(ix)
            .map(|p| p.collapsed.is_some())
            .unwrap_or(false)
    }

    /// Returns the expanded size of panel `ix`: the saved pre-collapse size if collapsed,
    /// or the current size otherwise. Used by dump to persist the meaningful size.
    pub fn expanded_size(&self, ix: usize) -> Pixels {
        self.panels
            .get(ix)
            .and_then(|p| p.collapsed)
            .unwrap_or_else(|| self.sizes.get(ix).copied().unwrap_or(PANEL_MIN_SIZE))
    }

    /// Mark panel `ix` as collapsed at load time, saving the current `sizes[ix]` as the
    /// expanded size and shrinking it to `strip`. Unlike `collapse_panel`, this does not
    /// redistribute sibling sizes (which would require valid bounds) and is safe to call
    /// before the first render.
    pub fn mark_collapsed(&mut self, ix: usize, strip: Pixels, cx: &mut Context<Self>) {
        if ix >= self.sizes.len() || self.panels[ix].collapsed.is_some() {
            return;
        }
        self.panels[ix].collapsed = Some(self.sizes[ix]);
        self.sizes[ix] = strip;
        self.panels[ix].size = Some(strip);
        cx.notify();
    }

    /// Resize the panel at `ix` by treating `ix` as the drag-handle position
    /// (the handle that sits between panel `ix` and panel `ix + 1`). Returns
    /// early on the last panel since there is no handle below it.
    ///
    /// This is the worker behind drag interactions and the public
    /// [`Self::resize_panel`] API.
    fn resize_panel_at_handle(
        &mut self,
        ix: usize,
        size: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.resize_panel_at_handle_inner(ix, size, None, window, cx);
    }

    /// Inner resize worker.
    ///
    /// `collapse_target` names a panel index that is allowed to shrink below
    /// its `size_range.start` (used by [`Self::collapse_panel`] to permit
    /// sub-[`PANEL_MIN_SIZE`] strip sizes). Pass `None` for normal drags.
    fn resize_panel_at_handle_inner(
        &mut self,
        ix: usize,
        size: Pixels,
        collapse_target: Option<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let old_sizes = self.sizes.clone();

        let mut ix = ix;
        // Only resize the left panels.
        if ix >= old_sizes.len() - 1 {
            return;
        }
        let container_size = self.container_size();
        self.sync_real_panel_sizes(cx);

        let move_changed = size - old_sizes[ix];
        if move_changed == px(0.) {
            return;
        }

        let size_range = self.panel_size_range(ix);
        // For the collapse target, allow the floor to be px(0.) so the strip
        // can go below the panel's normal minimum.
        let min_for_main = if collapse_target == Some(ix) {
            px(0.)
        } else {
            size_range.start
        };
        let new_size = size.clamp(min_for_main, size_range.end);
        let is_expand = move_changed > px(0.);

        let main_ix = ix;
        let mut new_sizes = old_sizes.clone();

        if is_expand {
            let mut changed = new_size - old_sizes[ix];
            new_sizes[ix] = new_size;

            while changed > px(0.) && ix < old_sizes.len() - 1 {
                ix += 1;
                let size_range = self.panel_size_range(ix);
                // If this sibling is the collapse target, it can shrink to 0.
                let min_for_ix = if collapse_target == Some(ix) {
                    px(0.)
                } else {
                    size_range.start
                };
                let available_size = (new_sizes[ix] - min_for_ix).max(px(0.));
                let to_reduce = changed.min(available_size);
                new_sizes[ix] -= to_reduce;
                changed -= to_reduce;
            }
        } else {
            let mut changed = new_size - size;
            new_sizes[ix] = new_size;

            while changed > px(0.) && ix > 0 {
                ix -= 1;
                let size_range = self.panel_size_range(ix);
                let min_for_ix = if collapse_target == Some(ix) {
                    px(0.)
                } else {
                    size_range.start
                };
                let available_size = (new_sizes[ix] - min_for_ix).max(px(0.));
                let to_reduce = changed.min(available_size);
                changed -= to_reduce;
                new_sizes[ix] -= to_reduce;
            }

            new_sizes[main_ix + 1] += old_sizes[main_ix] - size - changed;
        }

        // If total size exceeds container size, adjust the main panel.
        let total_size: Pixels = new_sizes.iter().map(|s| s.as_f32()).sum::<f32>().into();
        if total_size > container_size {
            let overflow = total_size - container_size;
            let floor = if collapse_target == Some(main_ix) {
                px(0.)
            } else {
                size_range.start
            };
            new_sizes[main_ix] = (new_sizes[main_ix] - overflow).max(floor);
        }

        for (i, _) in old_sizes.iter().enumerate() {
            let size = new_sizes[i];
            self.panels[i].size = Some(size);
        }
        self.sizes = new_sizes;
        cx.notify();
    }

    /// Adjust panel sizes according to the container size.
    ///
    /// When the container size changes, the panels should take up the same percentage as they did
    /// before. Collapsed panels keep their strip size; only non-collapsed panels are scaled.
    fn adjust_to_container_size(&mut self, cx: &mut Context<Self>) {
        if self.container_size().is_zero() {
            return;
        }

        let container_size = self.container_size();

        // Collapsed panels retain their strip size; distribute the remaining space.
        let collapsed_total: f32 = self
            .panels
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.collapsed.is_some().then(|| self.sizes[i].as_f32()))
            .sum();
        let available = (container_size.as_f32() - collapsed_total).max(0.);
        let non_collapsed_total: f32 = self
            .sizes
            .iter()
            .enumerate()
            .filter(|(i, _)| self.panels[*i].collapsed.is_none())
            .map(|(_, s)| s.as_f32())
            .sum::<f32>()
            .max(1.);

        for i in 0..self.panels.len() {
            if self.panels[i].collapsed.is_some() {
                self.panels[i].size = Some(self.sizes[i]);
            } else {
                let new_size = px(available * (self.sizes[i].as_f32() / non_collapsed_total));
                self.sizes[i] = new_size;
                self.panels[i].size = Some(new_size);
            }
        }
        cx.notify();
    }
}

impl EventEmitter<ResizablePanelEvent> for ResizableState {}

#[derive(Debug, Clone, Default)]
pub(crate) struct ResizablePanelState {
    pub size: Option<Pixels>,
    pub size_range: Range<Pixels>,
    bounds: Bounds<Pixels>,
    /// Saved expanded size when the panel is collapsed; `None` means not collapsed.
    pub(crate) collapsed: Option<Pixels>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Bounds, Entity, Point, Size, TestAppContext};

    /// Build a `ResizableState` with two equal horizontal panels of 300 px each
    /// inside a 600 × 400 container. Panel bounds are set so that
    /// `sync_real_panel_sizes` reads back the correct initial sizes.
    fn make_two_panel_state(cx: &mut gpui::App) -> Entity<ResizableState> {
        let panel_size = px(300.);
        cx.new(|_| ResizableState {
            axis: Axis::Horizontal,
            panels: vec![
                ResizablePanelState {
                    size: Some(panel_size),
                    size_range: PANEL_MIN_SIZE..Pixels::MAX,
                    bounds: Bounds {
                        origin: Point::default(),
                        size: Size {
                            width: panel_size,
                            height: px(400.),
                        },
                    },
                    collapsed: None,
                },
                ResizablePanelState {
                    size: Some(panel_size),
                    size_range: PANEL_MIN_SIZE..Pixels::MAX,
                    bounds: Bounds {
                        origin: Point {
                            x: panel_size,
                            y: px(0.),
                        },
                        size: Size {
                            width: panel_size,
                            height: px(400.),
                        },
                    },
                    collapsed: None,
                },
            ],
            sizes: vec![panel_size, panel_size],
            resizing_panel_ix: None,
            bounds: Bounds {
                origin: Point::default(),
                size: Size {
                    width: px(600.),
                    height: px(400.),
                },
            },
        })
    }

    /// Collapse the last panel (index 1) and verify sizes and is_collapsed.
    /// Then expand and verify restoration.
    #[gpui::test]
    fn test_collapse_and_expand_last_panel(cx: &mut TestAppContext) {
        let vcx = cx.add_empty_window();
        let state_entity = vcx.update(|_, cx| make_two_panel_state(cx));

        // Collapse panel 1 to a 28 px strip.
        vcx.update(|window, cx| {
            state_entity.update(cx, |state, cx| {
                state.collapse_panel(1, px(28.), window, cx);
            });
        });

        vcx.update(|_, cx| {
            let sizes = state_entity.read(cx).sizes().clone();
            assert!(
                (sizes[0].as_f32() - 572.0).abs() < 1.0,
                "expected sizes[0] ≈ 572, got {:?}",
                sizes[0]
            );
            assert!(
                (sizes[1].as_f32() - 28.0).abs() < 1.0,
                "expected sizes[1] ≈ 28, got {:?}",
                sizes[1]
            );
            assert!(
                state_entity.read(cx).is_collapsed(1),
                "panel 1 should be marked collapsed"
            );
        });

        // Expand panel 1 back to its saved size.
        vcx.update(|window, cx| {
            state_entity.update(cx, |state, cx| {
                state.expand_panel(1, window, cx);
            });
        });

        vcx.update(|_, cx| {
            let sizes = state_entity.read(cx).sizes().clone();
            // Tolerance of 2 px covers any floating-point rounding.
            assert!(
                (sizes[0].as_f32() - 300.0).abs() < 2.0,
                "expected sizes[0] ≈ 300, got {:?}",
                sizes[0]
            );
            assert!(
                (sizes[1].as_f32() - 300.0).abs() < 2.0,
                "expected sizes[1] ≈ 300, got {:?}",
                sizes[1]
            );
            assert!(
                !state_entity.read(cx).is_collapsed(1),
                "panel 1 should be expanded"
            );
        });
    }

    /// Collapsing the first panel (index 0) must give its freed space to the
    /// right sibling (panel 1).
    #[gpui::test]
    fn test_collapse_first_panel(cx: &mut TestAppContext) {
        let vcx = cx.add_empty_window();
        let state_entity = vcx.update(|_, cx| make_two_panel_state(cx));

        vcx.update(|window, cx| {
            state_entity.update(cx, |state, cx| {
                state.collapse_panel(0, px(28.), window, cx);
            });
        });

        vcx.update(|_, cx| {
            let sizes = state_entity.read(cx).sizes().clone();
            assert!(
                (sizes[0].as_f32() - 28.0).abs() < 1.0,
                "expected sizes[0] ≈ 28, got {:?}",
                sizes[0]
            );
            assert!(
                (sizes[1].as_f32() - 572.0).abs() < 1.0,
                "expected sizes[1] ≈ 572, got {:?}",
                sizes[1]
            );
            assert!(state_entity.read(cx).is_collapsed(0));
            assert!(!state_entity.read(cx).is_collapsed(1));
        });
    }

    /// A second `collapse_panel` call on an already-collapsed panel is a no-op:
    /// sizes must not change and the saved size must remain the original.
    #[gpui::test]
    fn test_collapse_noop_already_collapsed(cx: &mut TestAppContext) {
        let vcx = cx.add_empty_window();
        let state_entity = vcx.update(|_, cx| make_two_panel_state(cx));

        // First collapse.
        vcx.update(|window, cx| {
            state_entity.update(cx, |state, cx| {
                state.collapse_panel(1, px(28.), window, cx);
            });
        });

        // Second collapse with a different strip size — must be a no-op.
        vcx.update(|window, cx| {
            state_entity.update(cx, |state, cx| {
                state.collapse_panel(1, px(10.), window, cx);
            });
        });

        vcx.update(|_, cx| {
            let sizes = state_entity.read(cx).sizes().clone();
            assert!(
                (sizes[1].as_f32() - 28.0).abs() < 1.0,
                "second collapse must be no-op; expected sizes[1] ≈ 28, got {:?}",
                sizes[1]
            );
            assert!(state_entity.read(cx).is_collapsed(1));
        });
    }

    /// An out-of-range index must be silently ignored.
    #[gpui::test]
    fn test_collapse_out_of_range_noop(cx: &mut TestAppContext) {
        let vcx = cx.add_empty_window();
        let state_entity = vcx.update(|_, cx| make_two_panel_state(cx));

        vcx.update(|window, cx| {
            state_entity.update(cx, |state, cx| {
                state.collapse_panel(5, px(28.), window, cx); // out of range
            });
        });

        vcx.update(|_, cx| {
            assert_eq!(state_entity.read(cx).sizes().len(), 2);
            assert!(!state_entity.read(cx).is_collapsed(0));
            assert!(!state_entity.read(cx).is_collapsed(1));
        });
    }

    /// Render-level regression: a collapsed panel must actually shrink to its
    /// strip size in layout, not just in `sizes`. The collapse math lives in
    /// `ResizableState`, but the flex floor (`min_w`/`min_h` + `flex_basis`)
    /// is applied in `ResizablePanel::render`. Without a collapsed-floor
    /// override there, the layout clamped the pane back up to `PANEL_MIN_SIZE`
    /// (≈100 px) — so the "collapsed" pane kept rendering its body even though
    /// `sizes[ix]` was 28. This test draws a real split and asserts the
    /// collapsed panel's rendered bounds are ~28 px (it was ≥100 pre-fix).
    #[gpui::test]
    fn test_collapsed_panel_renders_at_strip_size(cx: &mut TestAppContext) {
        use gpui::{IntoElement, ParentElement as _, Render, Styled as _, VisualTestContext, div};

        cx.update(crate::init);

        // Build the shared state up front (2 × 300 px in a 600 × 400 container)
        // so we can read panel bounds back after rendering.
        let state_entity = cx.update(|cx| make_two_panel_state(cx));

        struct CollapseRenderRoot {
            state: Entity<ResizableState>,
        }
        impl Render for CollapseRenderRoot {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(600.)).h(px(400.)).child(
                    h_resizable("collapse-render-test")
                        .with_state(&self.state)
                        .child(resizable_panel().child(div().child("left")))
                        .child(resizable_panel().child(div().child("right"))),
                )
            }
        }

        let state_for_root = state_entity.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let content = cx.new(|_| CollapseRenderRoot {
                state: state_for_root,
            });
            crate::Root::new(content, window, cx)
        });
        let cx: &mut VisualTestContext = cx;

        // First draw establishes panel bounds at the expanded sizes.
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        // Collapse the right panel to a 28 px strip and redraw.
        cx.update(|window, cx| {
            state_entity.update(cx, |state, cx| {
                state.collapse_panel(1, px(28.), window, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let width = cx.update(|_, cx| state_entity.read(cx).panels[1].bounds.size.width);
        assert!(
            width.as_f32() < 40.0,
            "collapsed panel should render at ~28 px, got {:?} (the flex floor \
             clamped it to PANEL_MIN_SIZE before the fix)",
            width
        );
    }
}
