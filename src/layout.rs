//! A small constraint solver that splits a [`Rect`] into sub-rectangles.
//!
//! This is the layout primitive most TUIs are built on: describe a region as a
//! stack of [`Constraint`]s along a [`Direction`], and [`Layout::split`] hands
//! back one rectangle per constraint. Fixed sizes are honored first; any slack
//! is handed to [`Constraint::Fill`] cells (weighted), and if a layout is
//! over-committed the excess is trimmed from the end.
//!
//! ```
//! use noroi::geom::Rect;
//! use noroi::layout::{Constraint, Direction, Layout};
//!
//! let area = Rect::new(0, 0, 40, 10);
//! let rows = Layout::new(Direction::Vertical)
//!     .constraints([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)])
//!     .split(area);
//! assert_eq!(rows[0].height, 1);   // header
//! assert_eq!(rows[1].height, 8);   // body absorbs the rest
//! assert_eq!(rows[2].height, 1);   // footer
//! ```

use crate::geom::Rect;
use alloc::vec;
use alloc::vec::Vec;

/// The axis along which a [`Layout`] divides its area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Split into columns, left to right.
    Horizontal,
    /// Split into rows, top to bottom.
    Vertical,
}

/// A sizing rule for one slot of a [`Layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint {
    /// Exactly this many cells.
    Length(u16),
    /// This percentage (0–100) of the available length.
    Percentage(u16),
    /// A fraction `numerator / denominator` of the available length.
    Ratio(u32, u32),
    /// At least this many cells; grows to absorb slack when no [`Fill`](Constraint::Fill) is present.
    Min(u16),
    /// At most this many cells.
    Max(u16),
    /// Take a share of the leftover space, weighted against other fills.
    Fill(u16),
}

/// A configured split: a direction, a list of constraints, and optional margin
/// and inter-slot spacing.
#[derive(Debug, Clone)]
pub struct Layout {
    direction: Direction,
    constraints: Vec<Constraint>,
    margin: u16,
    spacing: u16,
}

impl Layout {
    /// Start a layout in the given direction with no constraints.
    pub fn new(direction: Direction) -> Self {
        Layout {
            direction,
            constraints: Vec::new(),
            margin: 0,
            spacing: 0,
        }
    }

    /// Shorthand for a horizontal (column) layout.
    pub fn horizontal() -> Self {
        Layout::new(Direction::Horizontal)
    }

    /// Shorthand for a vertical (row) layout.
    pub fn vertical() -> Self {
        Layout::new(Direction::Vertical)
    }

    /// Replace the constraint list.
    pub fn constraints<I>(mut self, constraints: I) -> Self
    where
        I: IntoIterator<Item = Constraint>,
    {
        self.constraints = constraints.into_iter().collect();
        self
    }

    /// Inset the whole area by `margin` cells on every side before splitting.
    pub fn margin(mut self, margin: u16) -> Self {
        self.margin = margin;
        self
    }

    /// Leave `spacing` blank cells between adjacent slots.
    pub fn spacing(mut self, spacing: u16) -> Self {
        self.spacing = spacing;
        self
    }

    /// Split `area` into one [`Rect`] per constraint.
    pub fn split(&self, area: Rect) -> Vec<Rect> {
        let area = area.inner(self.margin);
        let n = self.constraints.len();
        if n == 0 {
            return Vec::new();
        }
        let total = match self.direction {
            Direction::Horizontal => area.width,
            Direction::Vertical => area.height,
        };
        let gaps = self.spacing.saturating_mul((n as u16).saturating_sub(1));
        let usable = total.saturating_sub(gaps);
        let sizes = solve(usable, &self.constraints);

        let mut out = Vec::with_capacity(n);
        let mut cursor = match self.direction {
            Direction::Horizontal => area.x,
            Direction::Vertical => area.y,
        };
        for (i, &size) in sizes.iter().enumerate() {
            let rect = match self.direction {
                Direction::Horizontal => Rect::new(cursor, area.y, size, area.height),
                Direction::Vertical => Rect::new(area.x, cursor, area.width, size),
            };
            out.push(rect);
            cursor = cursor.saturating_add(size).saturating_add(self.spacing);
            let _ = i;
        }
        out
    }
}

/// Resolve constraints against a total length, returning one size per constraint.
fn solve(total: u16, constraints: &[Constraint]) -> Vec<u16> {
    let n = constraints.len();
    let total = total as u32;
    let mut sizes = vec![0u32; n];
    let mut fill_weights = vec![0u32; n];
    let mut any_fill = false;

    for (i, c) in constraints.iter().enumerate() {
        sizes[i] = match *c {
            Constraint::Length(v) => v as u32,
            Constraint::Percentage(p) => (p as u32 * total).div_ceil(100).min(total),
            Constraint::Ratio(a, b) => (a * total).checked_div(b).unwrap_or(0).min(total),
            Constraint::Min(v) => v as u32,
            Constraint::Max(v) => v as u32,
            Constraint::Fill(w) => {
                any_fill = true;
                fill_weights[i] = w as u32;
                0
            }
        };
    }

    let used: u32 = sizes.iter().sum();

    if used < total {
        let mut extra = total - used;
        if any_fill {
            // `.max(1)` keeps the divisor safe when every fill has weight 0
            // (in which case the loops below simply distribute nothing).
            let weight_sum: u32 = fill_weights.iter().sum::<u32>().max(1);
            {
                // Largest-remainder apportionment for fairness.
                let mut remainders: Vec<(usize, u32)> = Vec::new();
                let mut distributed: u32 = 0;
                for (i, &w) in fill_weights.iter().enumerate() {
                    if w == 0 {
                        continue;
                    }
                    let exact = extra * w;
                    let base = exact / weight_sum;
                    sizes[i] += base;
                    distributed += base;
                    remainders.push((i, exact % weight_sum));
                }
                remainders.sort_by_key(|&(_, r)| core::cmp::Reverse(r));
                let mut ri = 0;
                while distributed < extra && !remainders.is_empty() {
                    let (idx, _) = remainders[ri % remainders.len()];
                    sizes[idx] += 1;
                    distributed += 1;
                    ri += 1;
                }
            }
        } else {
            // No fills: hand slack to Min slots (they are allowed to grow),
            // else grow the last slot so the layout still covers the area.
            let growable: Vec<usize> = constraints
                .iter()
                .enumerate()
                .filter(|(_, c)| matches!(c, Constraint::Min(_)))
                .map(|(i, _)| i)
                .collect();
            if !growable.is_empty() {
                let each = extra / growable.len() as u32;
                let mut rem = extra % growable.len() as u32;
                for &i in &growable {
                    sizes[i] += each;
                    if rem > 0 {
                        sizes[i] += 1;
                        rem -= 1;
                    }
                }
            } else if n > 0 {
                sizes[n - 1] += extra;
            }
            extra = 0;
            let _ = extra;
        }
    } else if used > total {
        // Over-committed: trim from the end, each slot down to zero.
        let mut deficit = used - total;
        for i in (0..n).rev() {
            if deficit == 0 {
                break;
            }
            let take = sizes[i].min(deficit);
            sizes[i] -= take;
            deficit -= take;
        }
    }

    // Apply Max ceilings after distribution, redistributing any clipped excess
    // is intentionally skipped to keep behavior predictable.
    for (i, c) in constraints.iter().enumerate() {
        if let Constraint::Max(v) = *c {
            sizes[i] = sizes[i].min(v as u32);
        }
    }

    sizes
        .into_iter()
        .map(|v| v.min(u16::MAX as u32) as u16)
        .collect()
}

// ============================================================================
// Higher-level ergonomic layout: rows, columns, spacers and grids.
// ============================================================================

/// A horizontal (column) layout: children laid left to right.
///
/// Sugar for `Layout::horizontal().constraints(..)`.
pub fn row<I: IntoIterator<Item = Constraint>>(constraints: I) -> Layout {
    Layout::horizontal().constraints(constraints)
}

/// A vertical (row) layout: children laid top to bottom.
///
/// Sugar for `Layout::vertical().constraints(..)`.
pub fn column<I: IntoIterator<Item = Constraint>>(constraints: I) -> Layout {
    Layout::vertical().constraints(constraints)
}

/// A flexible gap that soaks up leftover space — a [`Constraint::Fill`] of
/// weight 1. Use it to push siblings apart or to center a fixed-size child.
///
/// ```
/// use noroi::geom::Rect;
/// use noroi::layout::{column, spacer, Constraint};
///
/// // Vertically center a 3-row panel.
/// let rows = column([spacer(), Constraint::Length(3), spacer()]).split(Rect::new(0, 0, 10, 9));
/// assert_eq!(rows[1], Rect::new(0, 3, 10, 3));
/// ```
pub const fn spacer() -> Constraint {
    Constraint::Fill(1)
}

/// A fixed-size gap of `n` cells — a [`Constraint::Length`].
pub const fn gap(n: u16) -> Constraint {
    Constraint::Length(n)
}

/// A two-dimensional layout: split an area into a grid of rows and columns.
///
/// The grid resolves its row heights and column widths independently using the
/// same [`Constraint`] solver, then produces one [`Rect`] per cell.
///
/// ```
/// use noroi::geom::Rect;
/// use noroi::layout::{Constraint, Grid};
///
/// let cells = Grid::new()
///     .rows([Constraint::Fill(1), Constraint::Fill(1)])
///     .columns([Constraint::Length(4), Constraint::Fill(1)])
///     .split(Rect::new(0, 0, 10, 4));
/// assert_eq!(cells.get(0, 0), Some(Rect::new(0, 0, 4, 2)));
/// assert_eq!(cells.get(1, 1), Some(Rect::new(4, 2, 6, 2)));
/// ```
#[derive(Debug, Clone, Default)]
pub struct Grid {
    rows: Vec<Constraint>,
    columns: Vec<Constraint>,
    margin: u16,
    col_spacing: u16,
    row_spacing: u16,
}

impl Grid {
    /// An empty grid.
    pub fn new() -> Self {
        Grid::default()
    }

    /// Set the row constraints (vertical division).
    pub fn rows<I: IntoIterator<Item = Constraint>>(mut self, rows: I) -> Self {
        self.rows = rows.into_iter().collect();
        self
    }

    /// Set the column constraints (horizontal division).
    pub fn columns<I: IntoIterator<Item = Constraint>>(mut self, columns: I) -> Self {
        self.columns = columns.into_iter().collect();
        self
    }

    /// Inset the whole grid before splitting.
    pub fn margin(mut self, margin: u16) -> Self {
        self.margin = margin;
        self
    }

    /// Blank cells between columns and between rows.
    pub fn spacing(mut self, column_spacing: u16, row_spacing: u16) -> Self {
        self.col_spacing = column_spacing;
        self.row_spacing = row_spacing;
        self
    }

    /// Resolve the grid over `area`, returning all cell rectangles.
    pub fn split(&self, area: Rect) -> GridCells {
        let row_rects = Layout::vertical()
            .constraints(self.rows.iter().copied())
            .margin(self.margin)
            .spacing(self.row_spacing)
            .split(area);
        let ncols = self.columns.len();
        let nrows = row_rects.len();
        let mut cells = Vec::with_capacity(nrows * ncols);
        for row_rect in row_rects {
            let col_rects = Layout::horizontal()
                .constraints(self.columns.iter().copied())
                .spacing(self.col_spacing)
                .split(row_rect);
            cells.extend(col_rects);
        }
        GridCells {
            cells,
            nrows,
            ncols,
        }
    }
}

/// The resolved rectangles of a [`Grid`], addressable by `(row, column)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCells {
    cells: Vec<Rect>,
    nrows: usize,
    ncols: usize,
}

impl GridCells {
    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.nrows
    }

    /// Number of columns.
    pub fn columns(&self) -> usize {
        self.ncols
    }

    /// The rectangle at `(row, column)`, or `None` if out of range.
    pub fn get(&self, row: usize, column: usize) -> Option<Rect> {
        if row < self.nrows && column < self.ncols {
            self.cells.get(row * self.ncols + column).copied()
        } else {
            None
        }
    }

    /// All cell rectangles in row-major order.
    pub fn all(&self) -> &[Rect] {
        &self.cells
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_body_footer() {
        let s = solve(
            10,
            &[
                Constraint::Length(1),
                Constraint::Fill(1),
                Constraint::Length(1),
            ],
        );
        assert_eq!(s, vec![1, 8, 1]);
    }

    #[test]
    fn weighted_fill() {
        let s = solve(10, &[Constraint::Fill(1), Constraint::Fill(3)]);
        assert_eq!(s.iter().sum::<u16>(), 10);
        assert_eq!(s, vec![3, 7]); // 1:3 split of 10 → 2.5:7.5, rounded fairly
    }

    #[test]
    fn percentage() {
        let s = solve(
            100,
            &[Constraint::Percentage(25), Constraint::Percentage(75)],
        );
        assert_eq!(s, vec![25, 75]);
    }

    #[test]
    fn overcommit_trims_from_end() {
        let s = solve(5, &[Constraint::Length(4), Constraint::Length(4)]);
        assert_eq!(s, vec![4, 1]);
    }

    #[test]
    fn split_positions() {
        let rows = Layout::vertical()
            .constraints([Constraint::Length(2), Constraint::Fill(1)])
            .split(Rect::new(0, 0, 10, 6));
        assert_eq!(rows[0], Rect::new(0, 0, 10, 2));
        assert_eq!(rows[1], Rect::new(0, 2, 10, 4));
    }

    #[test]
    fn spacing_between_slots() {
        let cols = Layout::horizontal()
            .constraints([Constraint::Fill(1), Constraint::Fill(1)])
            .spacing(2)
            .split(Rect::new(0, 0, 12, 3));
        assert_eq!(cols[0], Rect::new(0, 0, 5, 3));
        assert_eq!(cols[1], Rect::new(7, 0, 5, 3));
    }

    #[test]
    fn spacer_centers() {
        let rows =
            column([spacer(), Constraint::Length(3), spacer()]).split(Rect::new(0, 0, 10, 9));
        assert_eq!(rows[1], Rect::new(0, 3, 10, 3));
    }

    #[test]
    fn grid_cells() {
        let cells = Grid::new()
            .rows([Constraint::Fill(1), Constraint::Fill(1)])
            .columns([Constraint::Length(4), Constraint::Fill(1)])
            .split(Rect::new(0, 0, 10, 4));
        assert_eq!(cells.rows(), 2);
        assert_eq!(cells.columns(), 2);
        assert_eq!(cells.get(0, 0), Some(Rect::new(0, 0, 4, 2)));
        assert_eq!(cells.get(0, 1), Some(Rect::new(4, 0, 6, 2)));
        assert_eq!(cells.get(1, 1), Some(Rect::new(4, 2, 6, 2)));
        assert_eq!(cells.get(2, 0), None);
    }
}
