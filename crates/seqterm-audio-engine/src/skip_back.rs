//! Skip-back buffer: a lock-free circular buffer holding the last N seconds
//! of stereo (interleaved L/R) audio output from the master mix.
//!
//! The RT callback writes into it every block without allocation or locking.
//! Any thread can request a snapshot of the last N samples via `capture()`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Lock-free circular buffer for skip-back capture.
/// Internally stores interleaved stereo f32 samples.
pub struct SkipBackBuffer {
    buf:    Vec<f32>,           // ring buffer (stereo interleaved)
    cap:    usize,              // capacity in frames (buf.len() / 2)
    head:   Arc<AtomicUsize>,   // write index (in frames, wraps at cap)
    filled: Arc<AtomicUsize>,   // how many valid frames are in buf
}

impl SkipBackBuffer {
    /// Create a new skip-back buffer holding `duration_secs` at `sample_rate` Hz.
    pub fn new(duration_secs: u32, sample_rate: u32) -> Self {
        let cap = (duration_secs as usize) * (sample_rate as usize);
        Self {
            buf:    vec![0.0f32; cap * 2],
            cap,
            head:   Arc::new(AtomicUsize::new(0)),
            filled: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Write one stereo frame (L, R) from the RT callback.
    /// This is the **only** write path; never allocates.
    #[inline]
    pub fn push(&mut self, l: f32, r: f32) {
        let h = self.head.load(Ordering::Relaxed);
        let idx = h * 2;
        self.buf[idx]     = l;
        self.buf[idx + 1] = r;
        let next = if h + 1 >= self.cap { 0 } else { h + 1 };
        self.head.store(next, Ordering::Release);
        let filled = self.filled.load(Ordering::Relaxed).min(self.cap - 1);
        self.filled.store((filled + 1).min(self.cap), Ordering::Release);
    }

    /// Write a block of interleaved stereo samples. Wraps correctly.
    pub fn push_block(&mut self, interleaved: &[f32]) {
        let frames = interleaved.len() / 2;
        for i in 0..frames {
            self.push(interleaved[i * 2], interleaved[i * 2 + 1]);
        }
    }

    /// Capture the last `frames` frames as a Vec<f32> (interleaved stereo).
    /// Returns fewer frames if less audio has been recorded.
    pub fn capture(&self, frames: usize) -> Vec<f32> {
        let head    = self.head.load(Ordering::Acquire);
        let filled  = self.filled.load(Ordering::Acquire);
        let frames  = frames.min(filled);
        if frames == 0 { return Vec::new(); }

        let mut out = vec![0.0f32; frames * 2];
        // Walk backwards from head
        let start = if head >= frames {
            head - frames
        } else {
            self.cap - (frames - head)
        };

        for i in 0..frames {
            let src = ((start + i) % self.cap) * 2;
            out[i * 2]     = self.buf[src];
            out[i * 2 + 1] = self.buf[src + 1];
        }
        out
    }

    /// Duration in frames currently recorded.
    pub fn filled_frames(&self) -> usize {
        self.filled.load(Ordering::Acquire)
    }

    pub fn capacity_frames(&self) -> usize {
        self.cap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_back_push_and_capture() {
        let mut buf = SkipBackBuffer::new(1, 100); // 100 frames
        for i in 0..50u32 {
            buf.push(i as f32, -(i as f32));
        }
        let cap = buf.capture(10);
        assert_eq!(cap.len(), 20); // 10 frames * 2 channels
        // Last frame should be (49, -49)
        assert_eq!(cap[cap.len() - 2], 49.0);
        assert_eq!(cap[cap.len() - 1], -49.0);
    }

    #[test]
    fn capture_more_than_filled_returns_all() {
        let mut buf = SkipBackBuffer::new(1, 100);
        buf.push(1.0, 2.0);
        let cap = buf.capture(50);
        assert_eq!(cap.len(), 2); // only 1 frame available
    }

    #[test]
    fn wraps_correctly() {
        let mut buf = SkipBackBuffer::new(1, 4); // tiny 4-frame ring
        for i in 0..8u32 {
            buf.push(i as f32, 0.0);
        }
        // Should have last 4 frames: 4,5,6,7
        let cap = buf.capture(4);
        assert_eq!(cap[0], 4.0);
        assert_eq!(cap[2], 5.0);
        assert_eq!(cap[4], 6.0);
        assert_eq!(cap[6], 7.0);
    }
}
