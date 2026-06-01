//! The Report_Renderer: PDF + HTML rendering with Markdown fallback (R19).

use serde::{Deserialize, Serialize};

/// Errors from the rendering pipeline.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// The template engine failed to produce HTML.
    #[error("template error: {0}")]
    Template(String),
    /// The PDF renderer failed to convert HTML to PDF.
    #[error("PDF render error: {0}")]
    Pdf(String),
    /// An I/O error writing the output file.
    #[error("render I/O error: {0}")]
    Io(String),
}

/// The format requested for the rendered output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    /// A formatted PDF document (R19.1).
    Pdf,
    /// An interactive HTML dashboard (R19.2).
    Html,
    /// Plain Markdown (the fallback, R19.4).
    Markdown,
}

/// A request to render agent output into a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderRequest {
    /// The desired output format.
    pub format: OutputFormat,
    /// The title of the report.
    pub title: String,
    /// The body content (Markdown text).
    pub body: String,
    /// Embedded assets (images, tables) as `(name, data_base64)` pairs.
    #[serde(default)]
    pub assets: Vec<(String, String)>,
}

impl RenderRequest {
    /// Creates a render request.
    pub fn new(format: OutputFormat, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            format,
            title: title.into(),
            body: body.into(),
            assets: Vec::new(),
        }
    }

    /// Adds an embedded asset.
    #[must_use]
    pub fn with_asset(mut self, name: impl Into<String>, data: impl Into<String>) -> Self {
        self.assets.push((name.into(), data.into()));
        self
    }
}

/// The rendered output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOutput {
    /// The format that was actually produced (may differ from requested if
    /// fallback was triggered, R19.4).
    pub format: OutputFormat,
    /// The rendered content bytes (PDF binary, HTML text, or Markdown text).
    pub content: Vec<u8>,
    /// Whether this output is a fallback (the requested format failed).
    pub is_fallback: bool,
}

/// Produces HTML from a template + data. Backed by Tera in production.
pub trait TemplateEngine: Send + Sync {
    /// Renders the request into an HTML string.
    ///
    /// # Errors
    /// Returns a template-specific error string on failure.
    fn render_html(&self, request: &RenderRequest) -> Result<String, String>;
}

/// Converts HTML to PDF bytes. Backed by headless Chromium in production.
pub trait PdfRenderer: Send + Sync {
    /// Converts an HTML string to PDF bytes.
    ///
    /// # Errors
    /// Returns a renderer-specific error string on failure.
    fn html_to_pdf(&self, html: &str) -> Result<Vec<u8>, String>;
}

/// The Report_Renderer: assembles content, renders to the requested format,
/// and falls back to Markdown on failure (R19.4).
pub struct ReportRenderer<T, P> {
    template: T,
    pdf: P,
}

impl<T: TemplateEngine, P: PdfRenderer> ReportRenderer<T, P> {
    /// Creates a renderer with the given template engine and PDF backend.
    pub fn new(template: T, pdf: P) -> Self {
        Self { template, pdf }
    }

    /// Renders a request, falling back to Markdown on any failure (R19.4).
    pub fn render(&self, request: &RenderRequest) -> RenderOutput {
        match request.format {
            OutputFormat::Pdf => self.render_pdf(request),
            OutputFormat::Html => self.render_html(request),
            OutputFormat::Markdown => self.render_markdown(request),
        }
    }

    fn render_pdf(&self, request: &RenderRequest) -> RenderOutput {
        // Try template → HTML → PDF.
        let html = match self.template.render_html(request) {
            Ok(h) => h,
            Err(_) => return self.fallback(request),
        };
        match self.pdf.html_to_pdf(&html) {
            Ok(bytes) => RenderOutput {
                format: OutputFormat::Pdf,
                content: bytes,
                is_fallback: false,
            },
            Err(_) => self.fallback(request),
        }
    }

    fn render_html(&self, request: &RenderRequest) -> RenderOutput {
        match self.template.render_html(request) {
            Ok(html) => RenderOutput {
                format: OutputFormat::Html,
                content: html.into_bytes(),
                is_fallback: false,
            },
            Err(_) => self.fallback(request),
        }
    }

    fn render_markdown(&self, request: &RenderRequest) -> RenderOutput {
        RenderOutput {
            format: OutputFormat::Markdown,
            content: format!("# {}\n\n{}", request.title, request.body).into_bytes(),
            is_fallback: false,
        }
    }

    /// The Markdown fallback (R19.4): delivers the underlying content when
    /// rendering the requested format fails.
    fn fallback(&self, request: &RenderRequest) -> RenderOutput {
        RenderOutput {
            format: OutputFormat::Markdown,
            content: format!("# {}\n\n{}", request.title, request.body).into_bytes(),
            is_fallback: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A template engine that always succeeds.
    struct OkTemplate;
    impl TemplateEngine for OkTemplate {
        fn render_html(&self, req: &RenderRequest) -> Result<String, String> {
            Ok(format!("<h1>{}</h1><p>{}</p>", req.title, req.body))
        }
    }

    /// A template engine that always fails.
    struct FailTemplate;
    impl TemplateEngine for FailTemplate {
        fn render_html(&self, _req: &RenderRequest) -> Result<String, String> {
            Err("template crash".to_owned())
        }
    }

    /// A PDF renderer that always succeeds.
    struct OkPdf;
    impl PdfRenderer for OkPdf {
        fn html_to_pdf(&self, html: &str) -> Result<Vec<u8>, String> {
            Ok(format!("PDF:{html}").into_bytes())
        }
    }

    /// A PDF renderer that always fails.
    struct FailPdf;
    impl PdfRenderer for FailPdf {
        fn html_to_pdf(&self, _html: &str) -> Result<Vec<u8>, String> {
            Err("chromium not found".to_owned())
        }
    }

    fn request(format: OutputFormat) -> RenderRequest {
        RenderRequest::new(format, "Report", "Some content here.")
    }

    #[test]
    fn pdf_render_succeeds_with_working_backends() {
        let renderer = ReportRenderer::new(OkTemplate, OkPdf);
        let output = renderer.render(&request(OutputFormat::Pdf));
        assert_eq!(output.format, OutputFormat::Pdf);
        assert!(!output.is_fallback);
        assert!(String::from_utf8_lossy(&output.content).contains("PDF:"));
    }

    #[test]
    fn html_render_succeeds_with_working_template() {
        let renderer = ReportRenderer::new(OkTemplate, FailPdf);
        let output = renderer.render(&request(OutputFormat::Html));
        assert_eq!(output.format, OutputFormat::Html);
        assert!(!output.is_fallback);
        assert!(String::from_utf8_lossy(&output.content).contains("<h1>Report</h1>"));
    }

    #[test]
    fn markdown_render_always_succeeds() {
        let renderer = ReportRenderer::new(FailTemplate, FailPdf);
        let output = renderer.render(&request(OutputFormat::Markdown));
        assert_eq!(output.format, OutputFormat::Markdown);
        assert!(!output.is_fallback);
        let text = String::from_utf8_lossy(&output.content);
        assert!(text.contains("# Report"));
        assert!(text.contains("Some content here."));
    }

    #[test]
    fn pdf_falls_back_to_markdown_on_template_failure() {
        let renderer = ReportRenderer::new(FailTemplate, OkPdf);
        let output = renderer.render(&request(OutputFormat::Pdf));
        assert_eq!(output.format, OutputFormat::Markdown);
        assert!(output.is_fallback);
    }

    #[test]
    fn pdf_falls_back_to_markdown_on_pdf_failure() {
        let renderer = ReportRenderer::new(OkTemplate, FailPdf);
        let output = renderer.render(&request(OutputFormat::Pdf));
        assert_eq!(output.format, OutputFormat::Markdown);
        assert!(output.is_fallback);
    }

    #[test]
    fn html_falls_back_to_markdown_on_template_failure() {
        let renderer = ReportRenderer::new(FailTemplate, OkPdf);
        let output = renderer.render(&request(OutputFormat::Html));
        assert_eq!(output.format, OutputFormat::Markdown);
        assert!(output.is_fallback);
    }

    #[test]
    fn assets_are_included_in_request() {
        let req =
            RenderRequest::new(OutputFormat::Html, "T", "B").with_asset("chart.png", "base64data");
        assert_eq!(req.assets.len(), 1);
        assert_eq!(req.assets[0].0, "chart.png");
    }
}
