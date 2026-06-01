//! Infographic and diagram generation (R30).
//!
//! Generates diagrams (Mermaid, Graphviz) and charts as image assets
//! embeddable in PDF/HTML reports and deliverable on the originating channel.
//! Falls back to text representation on failure (R30.4).

use serde::{Deserialize, Serialize};

/// The diagram format/language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagramFormat {
    /// Mermaid diagram syntax.
    Mermaid,
    /// Graphviz DOT syntax.
    Graphviz,
    /// A simple chart (bar/line/pie) described in JSON.
    Chart,
}

/// The result of rendering a diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramResult {
    /// The rendered image bytes (PNG/SVG), or `None` if rendering failed.
    pub image: Option<Vec<u8>>,
    /// The text fallback (the source code of the diagram, R30.4).
    pub text_fallback: String,
    /// Whether the image was successfully rendered.
    pub rendered: bool,
}

/// Renders diagrams from source code to image assets. Backed by Mermaid CLI /
/// Graphviz `dot` in production; the trait allows testing with a fake.
pub trait DiagramRenderer: Send + Sync {
    /// Renders a diagram source to image bytes.
    ///
    /// # Errors
    /// Returns an error string if the rendering tool is unavailable or the
    /// source is malformed.
    fn render(&self, format: DiagramFormat, source: &str) -> Result<Vec<u8>, String>;
}

/// A diagram renderer that always fails (used when no rendering tool is
/// installed). Returns the text fallback (R30.4).
#[derive(Debug, Clone, Copy, Default)]
pub struct FallbackRenderer;

impl DiagramRenderer for FallbackRenderer {
    fn render(&self, _format: DiagramFormat, _source: &str) -> Result<Vec<u8>, String> {
        Err("no diagram renderer available; using text fallback".to_owned())
    }
}

/// Generates a [`DiagramResult`] from source code, using the given renderer
/// and falling back to text on failure (R30.4).
pub fn generate_diagram(
    renderer: &dyn DiagramRenderer,
    format: DiagramFormat,
    source: &str,
) -> DiagramResult {
    match renderer.render(format, source) {
        Ok(image) => DiagramResult {
            image: Some(image),
            text_fallback: source.to_owned(),
            rendered: true,
        },
        Err(_) => DiagramResult {
            image: None,
            text_fallback: source.to_owned(),
            rendered: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OkRenderer;
    impl DiagramRenderer for OkRenderer {
        fn render(&self, _format: DiagramFormat, source: &str) -> Result<Vec<u8>, String> {
            Ok(format!("IMG:{source}").into_bytes())
        }
    }

    #[test]
    fn successful_render_produces_image() {
        let result = generate_diagram(&OkRenderer, DiagramFormat::Mermaid, "graph TD; A-->B");
        assert!(result.rendered);
        assert!(result.image.is_some());
        assert_eq!(result.text_fallback, "graph TD; A-->B");
    }

    #[test]
    fn fallback_renderer_produces_text_only() {
        let result = generate_diagram(
            &FallbackRenderer,
            DiagramFormat::Graphviz,
            "digraph { a -> b }",
        );
        assert!(!result.rendered);
        assert!(result.image.is_none());
        assert_eq!(result.text_fallback, "digraph { a -> b }");
    }

    #[test]
    fn all_formats_are_distinct() {
        let formats = [
            DiagramFormat::Mermaid,
            DiagramFormat::Graphviz,
            DiagramFormat::Chart,
        ];
        for (i, a) in formats.iter().enumerate() {
            for (j, b) in formats.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn diagram_format_serde_round_trip() {
        for fmt in [
            DiagramFormat::Mermaid,
            DiagramFormat::Graphviz,
            DiagramFormat::Chart,
        ] {
            let json = serde_json::to_string(&fmt).unwrap();
            let back: DiagramFormat = serde_json::from_str(&json).unwrap();
            assert_eq!(fmt, back);
        }
    }
}
