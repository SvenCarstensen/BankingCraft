//! Compatibility bridge for sctk-adwaita 0.10 (used by winit 0.30).
//!
//! Crossfont 0.9's breaking change is the fontconfig-sys 5 -> 6 dependency
//! upgrade. The title renderer's Rust API is unchanged. Re-export upstream
//! instead of carrying a fork, so Fontique and Crossfont share one binding.
//! Remove this patch when winit uses a compatible Crossfont version itself.
pub use crossfont_upstream::*;
