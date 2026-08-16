use std::ops::Range;

/// A text-editor style selection represented by two insertion boundaries.
///
/// Both offsets are half-open boundaries in the buffer. `anchor` stays fixed
/// while `active` moves during a Shift-selection. The selected bytes are the
/// half-open range between the two boundaries; equal boundaries represent a
/// caret with no selected bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    anchor: usize,
    active: usize,
}

impl Selection {
    /// Creates a selection from an anchor boundary to an active boundary.
    pub const fn new(anchor: usize, active: usize) -> Self {
        Self { anchor, active }
    }

    /// Creates a collapsed selection (a caret) at `offset`.
    pub const fn collapsed(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    /// Returns the fixed boundary from which the selection started.
    pub const fn anchor(self) -> usize {
        self.anchor
    }

    /// Returns the moving boundary at the active edge of the selection.
    pub const fn active(self) -> usize {
        self.active
    }

    /// Returns `true` when the selection contains no bytes.
    pub const fn is_collapsed(self) -> bool {
        self.anchor == self.active
    }

    /// Returns the normalized half-open byte range, or `None` for a caret.
    pub fn range(self) -> Option<Range<usize>> {
        let start = self.anchor.min(self.active);
        let end = self.anchor.max(self.active);
        (start < end).then_some(start..end)
    }

    /// Clamps both boundaries to the document boundary at `total`.
    pub fn clamped(self, total: usize) -> Self {
        Self::new(self.anchor.min(total), self.active.min(total))
    }
}

#[cfg(test)]
mod tests {
    use super::Selection;

    #[test]
    fn selection_is_half_open_and_direction_independent() {
        assert_eq!(Selection::new(2, 5).range(), Some(2..5));
        assert_eq!(Selection::new(5, 2).range(), Some(2..5));
        assert_eq!(Selection::collapsed(3).range(), None);
    }

    #[test]
    fn selection_clamps_at_document_boundary() {
        assert_eq!(Selection::new(4, 9).clamped(6), Selection::new(4, 6));
    }
}
