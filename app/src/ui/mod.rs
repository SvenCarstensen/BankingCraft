//! egui presentation and a UI-thread dispatcher for the existing banking services.
mod dialogs;
mod history;
mod icons;
mod pages;
mod render;
mod runtime;
pub mod state;
mod theme;
mod widgets;
pub use render::run;
pub use runtime::*;
pub use state::AppState;
use std::sync::Arc;
pub type SharedString = String;
pub type Color = egui::Color32;
pub type VecModel<T> = Vec<T>;
#[derive(Clone, Debug, PartialEq)]
pub struct ModelRc<T>(Arc<Vec<T>>);
impl<T> Default for ModelRc<T> {
    fn default() -> Self {
        Self(Arc::new(Vec::new()))
    }
}
impl<T> ModelRc<T> {
    pub fn as_slice(&self) -> &[T] { &self.0 }
    pub fn new(rows: Vec<T>) -> Self {
        Self(Arc::new(rows))
    }
}
pub trait Model<T: Clone> {
    fn row_count(&self) -> usize;
    fn row_data(&self, index: usize) -> Option<T>;
    fn iter(&self) -> impl Iterator<Item = T>;
}
impl<T: Clone> Model<T> for ModelRc<T> {
    fn row_count(&self) -> usize {
        self.0.len()
    }
    fn row_data(&self, index: usize) -> Option<T> {
        self.0.get(index).cloned()
    }
    fn iter(&self) -> impl Iterator<Item = T> {
        self.0.iter().cloned()
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rgba8Pixel;
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SharedPixelBuffer<P> {
    pub bytes: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    marker: std::marker::PhantomData<P>,
}
impl<P> SharedPixelBuffer<P> {
    pub fn clone_from_slice(bytes: &[u8], width: u32, height: u32) -> Self {
        Self {
            bytes: Arc::new(bytes.to_vec()),
            width,
            height,
            marker: Default::default(),
        }
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Image(pub SharedPixelBuffer<Rgba8Pixel>);
impl Image {
    pub fn from_rgba8(pixels: SharedPixelBuffer<Rgba8Pixel>) -> Self {
        Self(pixels)
    }
}

#[cfg(test)]
pub mod testing;
