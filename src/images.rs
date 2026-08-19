use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;

use crate::layout::MAX_CONTENT_WIDTH;
use crate::markdown::{Block, Document};

const MAX_IMAGE_ROWS: u16 = 18;

pub enum Asset {
    Ready {
        protocol: Protocol,
        source: Arc<image::DynamicImage>,
        cols: u16,
        rows: u16,
    },
    Missing,
}

impl std::fmt::Debug for Asset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Asset::Ready { cols, rows, .. } => f
                .debug_struct("Ready")
                .field("cols", cols)
                .field("rows", rows)
                .finish(),
            Asset::Missing => write!(f, "Missing"),
        }
    }
}

pub struct ImageStore {
    picker: Picker,
    assets: HashMap<String, Asset>,
    scrolled_protocols: HashMap<String, (usize, usize, Protocol)>,
}

impl std::fmt::Debug for ImageStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageStore")
            .field("assets", &self.assets)
            .field("scrolled_protocols", &self.scrolled_protocols.keys())
            .finish()
    }
}

impl ImageStore {
    pub fn new(picker: Picker) -> Self {
        Self {
            picker,
            assets: HashMap::new(),
            scrolled_protocols: HashMap::new(),
        }
    }

    pub fn load(&mut self, document_dir: Option<&Path>, document: &Document) {
        self.assets.clear();
        self.scrolled_protocols.clear();
        for block in &document.blocks {
            if let Block::Image { src, .. } = block
                && !self.assets.contains_key(src)
            {
                let asset = self.load_asset(document_dir, src);
                self.assets.insert(src.clone(), asset);
            }
        }
    }

    pub fn asset(&self, src: &str) -> Option<&Asset> {
        self.assets.get(src)
    }

    fn load_asset(&self, document_dir: Option<&Path>, src: &str) -> Asset {
        let Some(path) = resolve_path(document_dir, src) else {
            return Asset::Missing;
        };
        let Some(decoded) = image::ImageReader::open(&path)
            .ok()
            .and_then(|reader| reader.with_guessed_format().ok())
            .and_then(|reader| reader.decode().ok())
        else {
            return Asset::Missing;
        };

        let source = Arc::new(decoded);
        let (cols, rows) = self.cell_size(source.width(), source.height());
        let area = Rect::new(0, 0, cols, rows);
        let Ok(protocol) =
            self.picker
                .new_protocol(source.as_ref().clone(), area, Resize::Fit(None))
        else {
            return Asset::Missing;
        };

        Asset::Ready {
            protocol,
            source,
            cols,
            rows,
        }
    }

    pub fn protocol_for_scroll(
        &mut self,
        src: &str,
        skipped_rows: usize,
        visible_rows: usize,
    ) -> Option<&Protocol> {
        if skipped_rows == 0 {
            let Asset::Ready { protocol, .. } = self.assets.get(src)? else {
                return None;
            };
            let Asset::Ready { rows, .. } = self.assets.get(src)? else {
                return None;
            };
            if visible_rows >= usize::from(*rows) {
                return Some(protocol);
            }
        }

        let (source, cols, rows) = match self.assets.get(src)? {
            Asset::Ready {
                source, cols, rows, ..
            } => (Arc::clone(source), *cols, *rows),
            Asset::Missing => return None,
        };
        if skipped_rows >= usize::from(rows) || visible_rows == 0 {
            return None;
        }

        let visible_rows = visible_rows.min(usize::from(rows) - skipped_rows);

        let needs_refresh =
            self.scrolled_protocols
                .get(src)
                .is_none_or(|(cached_skip, cached_rows, _)| {
                    *cached_skip != skipped_rows || *cached_rows != visible_rows
                });
        if needs_refresh {
            let crop = crop_rows(
                source.as_ref(),
                skipped_rows as u16,
                rows,
                visible_rows as u16,
            );
            let area = Rect::new(0, 0, cols, visible_rows as u16);
            let protocol = self
                .picker
                .new_protocol(crop, area, Resize::Fit(None))
                .ok()?;
            self.scrolled_protocols
                .insert(src.to_string(), (skipped_rows, visible_rows, protocol));
        }

        self.scrolled_protocols
            .get(src)
            .map(|(_, _, protocol)| protocol)
    }

    fn cell_size(&self, width_px: u32, height_px: u32) -> (u16, u16) {
        let (font_width, font_height) = self.picker.font_size();
        let font_width = u32::from(font_width.max(1));
        let font_height = u32::from(font_height.max(1));
        let natural_cols = width_px.div_ceil(font_width);
        let natural_rows = height_px.div_ceil(font_height);
        let scale = f64::min(
            1.0,
            f64::min(
                MAX_CONTENT_WIDTH as f64 / natural_cols as f64,
                f64::from(MAX_IMAGE_ROWS) / natural_rows as f64,
            ),
        );
        let cols = ((natural_cols as f64 * scale).ceil() as u16).max(1);
        let rows = ((natural_rows as f64 * scale).ceil() as u16).max(1);
        (cols, rows)
    }
}

impl Default for ImageStore {
    fn default() -> Self {
        Self::new(Picker::halfblocks())
    }
}

fn resolve_path(document_dir: Option<&Path>, src: &str) -> Option<PathBuf> {
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") {
        return None;
    }
    let path = Path::new(src);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    Some(document_dir?.join(path))
}

fn crop_rows(
    image: &image::DynamicImage,
    skipped_rows: u16,
    total_rows: u16,
    visible_rows: u16,
) -> image::DynamicImage {
    let height = image.height();
    let start = height.saturating_mul(u32::from(skipped_rows)) / u32::from(total_rows);
    let end_rows = skipped_rows.saturating_add(visible_rows).min(total_rows);
    let end = height.saturating_mul(u32::from(end_rows)) / u32::from(total_rows);
    image.crop_imm(0, start, image.width(), end.saturating_sub(start).max(1))
}

#[cfg(test)]
mod tests {
    use super::{ImageStore, resolve_path};
    use std::path::{Path, PathBuf};

    #[test]
    fn resolves_relative_sources_against_the_document_directory() {
        let dir = Path::new("/workspace/docs");
        assert_eq!(
            resolve_path(Some(dir), "assets/logo.png"),
            Some(PathBuf::from("/workspace/docs/assets/logo.png"))
        );
    }

    #[test]
    fn ignores_remote_and_data_sources() {
        let dir = Path::new("/workspace/docs");
        assert_eq!(
            resolve_path(Some(dir), "https://example.com/logo.png"),
            None
        );
        assert_eq!(resolve_path(Some(dir), "data:image/png;base64,AAA"), None);
    }

    #[test]
    fn requires_a_document_directory_for_relative_sources() {
        assert_eq!(resolve_path(None, "assets/logo.png"), None);
        assert_eq!(
            resolve_path(None, "/absolute/logo.png"),
            Some(PathBuf::from("/absolute/logo.png"))
        );
    }

    #[test]
    fn caps_image_size_to_the_content_area() {
        let store = ImageStore::default();
        let (cols, rows) = store.cell_size(4000, 4000);
        assert!(cols <= 88);
        assert!(rows <= 18);
        assert!(cols >= 1 && rows >= 1);
    }
}
