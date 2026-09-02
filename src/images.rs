use std::collections::HashMap;
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use ratatui::layout::Size;
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::sliced::SlicedProtocol;

use crate::markdown::{Block, Document};

const MAX_IMAGE_ROWS: u16 = 18;

pub enum Asset {
    Ready {
        protocol: SlicedProtocol,
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
}

impl std::fmt::Debug for ImageStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageStore")
            .field("assets", &self.assets)
            .finish()
    }
}

impl ImageStore {
    pub fn new(picker: Picker) -> Self {
        Self {
            picker,
            assets: HashMap::new(),
        }
    }

    /// Decodes every image the document references, sized to `measure` — the
    /// reader's current text column, which moves with the terminal.
    pub fn load(&mut self, document_dir: Option<&Path>, document: &Document, measure: usize) {
        self.assets.clear();
        for block in &document.blocks {
            if let Block::Image { src, .. } = block
                && !self.assets.contains_key(src)
            {
                let asset = self.load_asset(document_dir, src, measure);
                self.assets.insert(src.clone(), asset);
            }
        }
    }

    pub fn asset(&self, src: &str) -> Option<&Asset> {
        self.assets.get(src)
    }

    fn load_asset(&self, document_dir: Option<&Path>, src: &str, measure: usize) -> Asset {
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

        let (cols, rows) = self.cell_size(decoded.width(), decoded.height(), measure);
        let size = Size::new(cols, rows);
        // `Resize::Fit(None)` resamples with nearest-neighbour, which leaves
        // visible stair-stepping on any diagonal or curved edge. Lanczos costs
        // a few milliseconds once per load and keeps outlines smooth.
        let Ok(protocol) = SlicedProtocol::new_with_resize(
            &self.picker,
            decoded,
            size,
            Resize::Fit(Some(FilterType::Lanczos3)),
        ) else {
            return Asset::Missing;
        };
        let size = protocol.size();

        Asset::Ready {
            protocol,
            cols: size.width,
            rows: size.height,
        }
    }

    fn cell_size(&self, width_px: u32, height_px: u32, measure: usize) -> (u16, u16) {
        let font_size = self.picker.font_size();
        let font_width = u32::from(font_size.width.max(1));
        let font_height = u32::from(font_size.height.max(1));
        let natural_cols = width_px.div_ceil(font_width);
        let natural_rows = height_px.div_ceil(font_height);
        let scale = f64::min(
            1.0,
            f64::min(
                measure as f64 / natural_cols as f64,
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use ratatui_image::picker::{Picker, ProtocolType};
    use ratatui_image::sliced::SlicedProtocol;

    use super::{Asset, ImageStore, resolve_path};
    use crate::markdown::Document;

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
        let (cols, rows) = store.cell_size(4000, 4000, 88);
        assert!(cols <= 88);
        assert!(rows <= 18);
        assert!(cols >= 1 && rows >= 1);
    }

    #[test]
    fn creates_a_single_sliced_kitty_protocol() {
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let mut store = ImageStore::new(picker);
        let document = Document::parse("![logo](assets/markr-logo.png)");
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        store.load(Some(root), &document, 80);

        let Some(Asset::Ready { protocol, .. }) = store.asset("assets/markr-logo.png") else {
            panic!("sliced image protocol");
        };
        assert!(matches!(protocol, SlicedProtocol::Kitty(_)));
    }
}
