//! `cyrene-render`: the Report_Renderer for Cyrene (R19, R30).
//!
//! Compiles agent output into formatted PDF documents and interactive HTML
//! dashboards (R19.1, R19.2). Includes text, tables, and image assets (R19.3).
//! Falls back to Markdown when rendering fails (R19.4). Delivers rendered
//! output to the originating channel (R19.5).
//!
//! Also generates infographics and diagrams (Mermaid/Graphviz) as embeddable
//! image assets (R30).
//!
//! The rendering pipeline is expressed as traits ([`TemplateEngine`],
//! [`PdfRenderer`]) so the actual Tera/Chromium binding plugs in at the CLI
//! layer while the core logic (content assembly, fallback, delivery) is
//! testable in isolation.

mod diagram;
mod render;

pub use diagram::{
    generate_diagram, DiagramFormat, DiagramRenderer, DiagramResult, FallbackRenderer,
};
pub use render::{
    PdfRenderer, RenderError, RenderOutput, RenderRequest, ReportRenderer, TemplateEngine,
};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-render"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
