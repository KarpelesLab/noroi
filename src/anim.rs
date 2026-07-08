//! Time-based animation primitives.
//!
//! The core is `#![no_std]` and cannot read a clock, so animations here are
//! **driven**: the application owns the clock, measures the seconds elapsed
//! between frames, and calls `advance(dt)` on each animator. The animator holds
//! the state (progress, phase) and yields a value the app hands to an otherwise
//! stateless widget — which keeps rendering pure and diff-friendly.
//!
//! * [`Tween`] eases a value from one number to another over a duration, and can
//!   be [`retarget`](Tween::retarget)ed mid-flight (e.g. a gauge gliding to a
//!   new percentage instead of jumping).
//! * [`Pulse`] is a looping 0→1→0 oscillator for breathing/blinking effects.
//! * [`Easing`] provides the usual polynomial curves (no `std` math needed).
//!
//! For a spinning "busy" indicator see [`Spinner`](crate::widget::Spinner),
//! which is frame-based rather than value-based.
//!
//! ```
//! use noroi::anim::{Easing, Tween};
//! let mut t = Tween::new(0.0, 1.0, 1.0, Easing::EaseOutCubic);
//! t.advance(0.5);                 // half a second in
//! assert!(t.value() > 0.5);       // eased-out: past the midpoint already
//! assert!(!t.finished());
//! t.advance(0.6);
//! assert!(t.finished() && (t.value() - 1.0).abs() < 1e-6);
//! ```

/// Interpolate linearly from `a` to `b` at `t` (clamped to `0..=1`).
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Easing curves mapping normalized time `0..=1` to eased progress `0..=1`.
///
/// All are polynomial, so they need no floating-point library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Easing {
    /// Constant speed.
    #[default]
    Linear,
    /// Accelerate from rest (quadratic).
    EaseInQuad,
    /// Decelerate to rest (quadratic).
    EaseOutQuad,
    /// Accelerate then decelerate (quadratic).
    EaseInOutQuad,
    /// Accelerate from rest (cubic — stronger).
    EaseInCubic,
    /// Decelerate to rest (cubic — stronger).
    EaseOutCubic,
    /// Accelerate then decelerate (cubic).
    EaseInOutCubic,
}

impl Easing {
    /// Apply the curve to `t` (clamped to `0..=1`).
    pub fn ease(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseInQuad => t * t,
            Easing::EaseOutQuad => t * (2.0 - t),
            Easing::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    let u = -2.0 * t + 2.0;
                    1.0 - (u * u) / 2.0
                }
            }
            Easing::EaseInCubic => t * t * t,
            Easing::EaseOutCubic => {
                let u = 1.0 - t;
                1.0 - u * u * u
            }
            Easing::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let u = -2.0 * t + 2.0;
                    1.0 - (u * u * u) / 2.0
                }
            }
        }
    }
}

/// Animates a single `f32` from a start value to a target over a fixed duration.
///
/// Advance it each frame with the elapsed seconds; read the eased current value
/// with [`value`](Tween::value). [`retarget`](Tween::retarget) starts a fresh
/// glide from wherever the value currently is, so repeated retargets stay smooth.
#[derive(Debug, Clone, Copy)]
pub struct Tween {
    from: f32,
    to: f32,
    duration: f32,
    elapsed: f32,
    easing: Easing,
}

impl Tween {
    /// A tween from `from` to `to` over `duration` seconds with `easing`.
    pub fn new(from: f32, to: f32, duration: f32, easing: Easing) -> Self {
        Tween {
            from,
            to,
            duration: duration.max(0.0),
            elapsed: 0.0,
            easing,
        }
    }

    /// A tween already resting at `value` (nothing to animate).
    pub fn settled(value: f32) -> Self {
        Tween {
            from: value,
            to: value,
            duration: 0.0,
            elapsed: 0.0,
            easing: Easing::Linear,
        }
    }

    /// Advance by `dt` seconds and return the new [`value`](Tween::value).
    pub fn advance(&mut self, dt: f32) -> f32 {
        self.elapsed = (self.elapsed + dt.max(0.0)).min(self.duration);
        self.value()
    }

    /// The current eased value.
    pub fn value(&self) -> f32 {
        if self.duration <= 0.0 {
            return self.to;
        }
        let t = (self.elapsed / self.duration).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * self.easing.ease(t)
    }

    /// The value this tween is heading toward.
    pub fn target(&self) -> f32 {
        self.to
    }

    /// True once the full duration has elapsed.
    pub fn finished(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// Glide to a new target starting from the current value.
    pub fn retarget(&mut self, to: f32) {
        self.from = self.value();
        self.to = to;
        self.elapsed = 0.0;
    }

    /// Jump immediately to `value` (used to honor reduced-motion).
    pub fn snap(&mut self, value: f32) {
        self.from = value;
        self.to = value;
        self.elapsed = self.duration;
    }
}

/// A looping oscillator producing a value that rises `0→1` then falls `1→0`
/// every `period` seconds — for breathing highlights, blinking cursors and the
/// like. The triangle wave is smoothed with an ease-in-out curve so it feels
/// organic rather than mechanical.
#[derive(Debug, Clone, Copy)]
pub struct Pulse {
    period: f32,
    elapsed: f32,
    easing: Easing,
}

impl Pulse {
    /// A pulse with the given full-cycle `period` in seconds.
    pub fn new(period: f32) -> Self {
        Pulse {
            period: period.max(0.0001),
            elapsed: 0.0,
            easing: Easing::EaseInOutQuad,
        }
    }

    /// Advance by `dt` seconds and return the new [`value`](Pulse::value).
    pub fn advance(&mut self, dt: f32) -> f32 {
        self.elapsed += dt.max(0.0);
        while self.elapsed >= self.period {
            self.elapsed -= self.period;
        }
        self.value()
    }

    /// The current value in `0.0..=1.0`.
    pub fn value(&self) -> f32 {
        let phase = self.elapsed / self.period; // 0..1
        let triangle = if phase < 0.5 {
            phase * 2.0
        } else {
            2.0 - phase * 2.0
        };
        self.easing.ease(triangle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tween_reaches_target() {
        let mut t = Tween::new(0.0, 10.0, 1.0, Easing::Linear);
        assert_eq!(t.value(), 0.0);
        t.advance(0.5);
        assert!((t.value() - 5.0).abs() < 1e-4);
        t.advance(1.0);
        assert!(t.finished());
        assert!((t.value() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn retarget_is_continuous() {
        let mut t = Tween::new(0.0, 1.0, 1.0, Easing::Linear);
        t.advance(0.5); // ~0.5
        let mid = t.value();
        t.retarget(0.0);
        // Immediately after retarget the value hasn't jumped.
        assert!((t.value() - mid).abs() < 1e-6);
    }

    #[test]
    fn easing_endpoints() {
        for e in [
            Easing::Linear,
            Easing::EaseInQuad,
            Easing::EaseOutQuad,
            Easing::EaseInOutQuad,
            Easing::EaseInCubic,
            Easing::EaseOutCubic,
            Easing::EaseInOutCubic,
        ] {
            assert!(e.ease(0.0).abs() < 1e-6, "{e:?} at 0");
            assert!((e.ease(1.0) - 1.0).abs() < 1e-6, "{e:?} at 1");
        }
    }

    #[test]
    fn pulse_oscillates() {
        let mut p = Pulse::new(1.0);
        assert!(p.value() < 0.01); // starts low
        p.advance(0.5);
        assert!(p.value() > 0.9); // peak at half period
        p.advance(0.5);
        assert!(p.value() < 0.01); // back to low
    }
}
