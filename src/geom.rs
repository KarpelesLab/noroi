//! Integer geometry primitives used throughout noroi.
//!
//! All coordinates are cell coordinates: column `x` grows rightward, row `y`
//! grows downward, and the origin `(0, 0)` is the top-left cell. Sizes are
//! measured in whole cells. Everything is `u16`, which comfortably covers any
//! real terminal while keeping [`Cell`](crate::buffer::Cell) grids compact.

/// A single cell position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Point {
    /// Column, zero-based, growing to the right.
    pub x: u16,
    /// Row, zero-based, growing downward.
    pub y: u16,
}

impl Point {
    /// The top-left origin.
    pub const ORIGIN: Point = Point { x: 0, y: 0 };

    /// Create a point at `(x, y)`.
    pub const fn new(x: u16, y: u16) -> Self {
        Point { x, y }
    }
}

impl From<(u16, u16)> for Point {
    fn from((x, y): (u16, u16)) -> Self {
        Point { x, y }
    }
}

/// A width/height pair measured in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Size {
    /// Number of columns.
    pub width: u16,
    /// Number of rows.
    pub height: u16,
}

impl Size {
    /// Create a size.
    pub const fn new(width: u16, height: u16) -> Self {
        Size { width, height }
    }

    /// The total number of cells (`width * height`).
    pub const fn area(self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    /// True when either dimension is zero.
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

impl From<(u16, u16)> for Size {
    fn from((width, height): (u16, u16)) -> Self {
        Size { width, height }
    }
}

/// An axis-aligned rectangle of cells.
///
/// A rectangle is defined by its top-left corner and its size. The right and
/// bottom edges are exclusive: a rect at `x = 0` with `width = 3` covers
/// columns `0, 1, 2` and [`right`](Rect::right) returns `3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    /// Left edge (inclusive).
    pub x: u16,
    /// Top edge (inclusive).
    pub y: u16,
    /// Width in cells.
    pub width: u16,
    /// Height in cells.
    pub height: u16,
}

impl Rect {
    /// The empty rectangle at the origin.
    pub const ZERO: Rect = Rect { x: 0, y: 0, width: 0, height: 0 };

    /// Create a rectangle.
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Rect { x, y, width, height }
    }

    /// Create a rectangle at the origin with the given size.
    pub const fn from_size(size: Size) -> Self {
        Rect { x: 0, y: 0, width: size.width, height: size.height }
    }

    /// The size of the rectangle.
    pub const fn size(self) -> Size {
        Size { width: self.width, height: self.height }
    }

    /// The exclusive right edge (`x + width`), saturating.
    pub const fn right(self) -> u16 {
        self.x.saturating_add(self.width)
    }

    /// The exclusive bottom edge (`y + height`), saturating.
    pub const fn bottom(self) -> u16 {
        self.y.saturating_add(self.height)
    }

    /// The top-left corner.
    pub const fn top_left(self) -> Point {
        Point { x: self.x, y: self.y }
    }

    /// Number of cells the rectangle covers.
    pub const fn area(self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    /// True when the rectangle has no cells.
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// True when `p` lies inside the rectangle.
    pub const fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// Shrink the rectangle by `margin` cells on every side.
    pub fn inner(self, margin: u16) -> Rect {
        let both = margin.saturating_mul(2);
        Rect {
            x: self.x.saturating_add(margin),
            y: self.y.saturating_add(margin),
            width: self.width.saturating_sub(both),
            height: self.height.saturating_sub(both),
        }
    }

    /// Shrink independently on each axis.
    pub fn shrink(self, dx: u16, dy: u16) -> Rect {
        Rect {
            x: self.x.saturating_add(dx),
            y: self.y.saturating_add(dy),
            width: self.width.saturating_sub(dx.saturating_mul(2)),
            height: self.height.saturating_sub(dy.saturating_mul(2)),
        }
    }

    /// The largest rectangle contained in both `self` and `other`.
    pub fn intersection(self, other: Rect) -> Rect {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        if x2 <= x1 || y2 <= y1 {
            Rect { x: x1, y: y1, width: 0, height: 0 }
        } else {
            Rect { x: x1, y: y1, width: x2 - x1, height: y2 - y1 }
        }
    }

    /// True when the two rectangles share at least one cell.
    pub fn intersects(self, other: Rect) -> bool {
        !self.intersection(other).is_empty()
    }

    /// A rectangle centered within `self` with the given size (clamped to fit).
    pub fn centered(self, size: Size) -> Rect {
        let w = size.width.min(self.width);
        let h = size.height.min(self.height);
        let x = self.x + (self.width - w) / 2;
        let y = self.y + (self.height - h) / 2;
        Rect { x, y, width: w, height: h }
    }
}

impl From<Size> for Rect {
    fn from(size: Size) -> Self {
        Rect::from_size(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_and_edges() {
        let r = Rect::new(2, 3, 4, 5);
        assert_eq!(r.right(), 6);
        assert_eq!(r.bottom(), 8);
        assert!(r.contains(Point::new(2, 3)));
        assert!(r.contains(Point::new(5, 7)));
        assert!(!r.contains(Point::new(6, 7)));
        assert!(!r.contains(Point::new(5, 8)));
    }

    #[test]
    fn intersection_math() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersection(b), Rect::new(5, 5, 5, 5));
        let c = Rect::new(20, 20, 2, 2);
        assert!(a.intersection(c).is_empty());
        assert!(!a.intersects(c));
    }

    #[test]
    fn centered_clamps() {
        let area = Rect::new(0, 0, 10, 4);
        assert_eq!(area.centered(Size::new(4, 2)), Rect::new(3, 1, 4, 2));
        assert_eq!(area.centered(Size::new(100, 100)), Rect::new(0, 0, 10, 4));
    }
}
