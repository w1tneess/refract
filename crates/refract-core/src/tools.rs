use crate::format::Kind;
use serde::{Deserialize, Serialize};

/// Which native engine backs a tool.
///
/// `Builtin` tools run entirely inside Refract. The others shell out to an
/// external binary that must be installed on the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolEngine {
    /// Pure Rust, no external dependency.
    Builtin,
    /// Requires `ffmpeg` / `ffprobe` on PATH.
    Ffmpeg,
    /// Requires a PDF rasterizer (`mutool`, `pdftoppm`, `pdftocairo` or `gs`).
    PdfRenderer,
}

impl ToolEngine {
    pub fn label(&self) -> &'static str {
        match self {
            ToolEngine::Builtin => "Built-in engine",
            ToolEngine::Ffmpeg => "FFmpeg",
            ToolEngine::PdfRenderer => "PDF renderer",
        }
    }

    /// Human readable name of the binary that has to be present.
    pub fn binary_hint(&self) -> &'static str {
        match self {
            ToolEngine::Builtin => "",
            ToolEngine::Ffmpeg => "ffmpeg",
            ToolEngine::PdfRenderer => "mutool / pdftoppm / pdftocairo / gs",
        }
    }
}

/// Static description of a tool: how it is presented and what it needs.
#[derive(Debug, Clone, Copy)]
pub struct ToolSpec {
    pub id: ToolId,
    pub title: &'static str,
    pub tagline: &'static str,
    pub description: &'static str,
    pub domain: Kind,
    /// Labels of the accepted input formats.
    pub input_formats: &'static [&'static str],
    /// What the tool produces.
    pub output_capability: &'static str,
    pub engine: ToolEngine,
    /// `true` when many inputs collapse into one output document.
    pub batch: bool,
    /// Minimum number of inputs required for the tool to run.
    pub min_files: usize,
    /// Maximum number of inputs accepted in a single run.
    pub max_files: Option<usize>,
    /// File extensions accepted by the file picker (without dot).
    pub extensions: &'static [&'static str],
}

impl ToolSpec {
    pub fn accepts_multiple(&self) -> bool {
        self.max_files.map(|m| m > 1).unwrap_or(self.batch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolId {
    ConvertImage,
    CompressImage,
    ResizeImage,
    MergePdf,
    SplitPdf,
    ReorderPdf,
    ImagesToPdf,
    PdfToImages,
    CompressPdf,
    ConvertAudio,
    TrimAudio,
}

impl ToolId {
    pub const ALL: &'static [ToolId] = &[
        ToolId::ConvertImage,
        ToolId::CompressImage,
        ToolId::ResizeImage,
        ToolId::MergePdf,
        ToolId::SplitPdf,
        ToolId::ReorderPdf,
        ToolId::ImagesToPdf,
        ToolId::PdfToImages,
        ToolId::CompressPdf,
        ToolId::ConvertAudio,
        ToolId::TrimAudio,
    ];

    pub fn id_str(&self) -> &'static str {
        match self {
            ToolId::ConvertImage => "convert-image",
            ToolId::CompressImage => "compress-image",
            ToolId::ResizeImage => "resize-image",
            ToolId::MergePdf => "merge-pdf",
            ToolId::SplitPdf => "split-pdf",
            ToolId::ReorderPdf => "reorder-pdf",
            ToolId::ImagesToPdf => "images-to-pdf",
            ToolId::PdfToImages => "pdf-to-images",
            ToolId::CompressPdf => "compress-pdf",
            ToolId::ConvertAudio => "convert-audio",
            ToolId::TrimAudio => "trim-audio",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ToolId::ConvertImage => "Convert image",
            ToolId::CompressImage => "Compress image",
            ToolId::ResizeImage => "Resize image",
            ToolId::MergePdf => "Merge PDF",
            ToolId::SplitPdf => "Split PDF",
            ToolId::ReorderPdf => "Reorder PDF pages",
            ToolId::ImagesToPdf => "Images to PDF",
            ToolId::PdfToImages => "PDF to images",
            ToolId::CompressPdf => "Compress PDF",
            ToolId::ConvertAudio => "Convert audio",
            ToolId::TrimAudio => "Trim audio",
        }
    }

    pub fn accepts(&self) -> Kind {
        match self {
            ToolId::ConvertImage | ToolId::CompressImage | ToolId::ResizeImage | ToolId::ImagesToPdf => Kind::Image,
            ToolId::MergePdf | ToolId::SplitPdf | ToolId::ReorderPdf | ToolId::PdfToImages | ToolId::CompressPdf => Kind::Pdf,
            ToolId::ConvertAudio | ToolId::TrimAudio => Kind::Audio,
        }
    }

    /// Full presentation + capability metadata for the tool.
    pub fn spec(&self) -> ToolSpec {
        match self {
            ToolId::ConvertImage => ToolSpec {
                id: *self,
                title: "Convert image",
                tagline: "Transcode between modern and standard image formats",
                description: "Convert between JPEG, PNG and WebP. Metadata is stripped and orientation is baked into the pixels.",
                domain: Kind::Image,
                input_formats: &["JPEG", "PNG", "WebP"],
                output_capability: "JPEG, PNG or WebP",
                engine: ToolEngine::Builtin,
                batch: false,
                min_files: 1,
                max_files: None,
                extensions: &["jpg", "jpeg", "png", "webp"],
            },
            ToolId::CompressImage => ToolSpec {
                id: *self,
                title: "Compress image",
                tagline: "Target-size compression using binary quality search",
                description: "Shrink an image to a target file size in kilobytes. Refract binary-searches the JPEG quality until it fits.",
                domain: Kind::Image,
                input_formats: &["JPEG", "PNG", "WebP"],
                output_capability: "JPEG under the requested size",
                engine: ToolEngine::Builtin,
                batch: false,
                min_files: 1,
                max_files: None,
                extensions: &["jpg", "jpeg", "png", "webp"],
            },
            ToolId::ResizeImage => ToolSpec {
                id: *self,
                title: "Resize image",
                tagline: "High-fidelity Lanczos-3 resampling by pixels or physical units",
                description: "Scale images by exact pixel dimensions or physical size in centimetres plus DPI, preserving the aspect ratio.",
                domain: Kind::Image,
                input_formats: &["JPEG", "PNG", "WebP"],
                output_capability: "Rescaled JPEG, PNG or WebP",
                engine: ToolEngine::Builtin,
                batch: false,
                min_files: 1,
                max_files: None,
                extensions: &["jpg", "jpeg", "png", "webp"],
            },
            ToolId::MergePdf => ToolSpec {
                id: *self,
                title: "Merge PDF",
                tagline: "Combine multiple PDF documents in custom drop sequence",
                description: "Combine every queued PDF into one document. The queue order becomes the page order, so you can reorder files before merging.",
                domain: Kind::Pdf,
                input_formats: &["PDF"],
                output_capability: "Single unified multi-page PDF",
                engine: ToolEngine::Builtin,
                batch: true,
                min_files: 2,
                max_files: None,
                extensions: &["pdf"],
            },
            ToolId::SplitPdf => ToolSpec {
                id: *self,
                title: "Split PDF",
                tagline: "Extract page ranges into separate documents",
                description: "Extract pages or page intervals (for example 1-3, 5, 8-end) into one PDF file per range.",
                domain: Kind::Pdf,
                input_formats: &["PDF"],
                output_capability: "One PDF per page range",
                engine: ToolEngine::Builtin,
                batch: false,
                min_files: 1,
                max_files: None,
                extensions: &["pdf"],
            },
            ToolId::ReorderPdf => ToolSpec {
                id: *self,
                title: "Reorder PDF pages",
                tagline: "Rearrange document pages with strict sequence validation",
                description: "Reorder pages with a custom sequence (for example 3, 1, 2). Every page has to appear exactly once.",
                domain: Kind::Pdf,
                input_formats: &["PDF"],
                output_capability: "Re-sequenced PDF document",
                engine: ToolEngine::Builtin,
                batch: false,
                min_files: 1,
                max_files: None,
                extensions: &["pdf"],
            },
            ToolId::ImagesToPdf => ToolSpec {
                id: *self,
                title: "Images to PDF",
                tagline: "Compile multiple image files into a single multi-page PDF",
                description: "Assemble JPEG, PNG and WebP images into one PDF, one page per image, sized to the original photo dimensions.",
                domain: Kind::Pdf,
                input_formats: &["JPEG", "PNG", "WebP"],
                output_capability: "Multi-page PDF document",
                engine: ToolEngine::Builtin,
                batch: true,
                min_files: 1,
                max_files: None,
                extensions: &["jpg", "jpeg", "png", "webp"],
            },
            ToolId::PdfToImages => ToolSpec {
                id: *self,
                title: "PDF to images",
                tagline: "Render document pages as full-resolution image files",
                description: "Rasterise PDF pages into PNG or JPEG images, one file per page. Rendering uses a local PDF rasterizer when one is installed.",
                domain: Kind::Pdf,
                input_formats: &["PDF"],
                output_capability: "One PNG or JPEG per page",
                engine: ToolEngine::PdfRenderer,
                batch: false,
                min_files: 1,
                max_files: None,
                extensions: &["pdf"],
            },
            ToolId::CompressPdf => ToolSpec {
                id: *self,
                title: "Compress PDF",
                tagline: "Lossless stream and object compression",
                description: "Losslessly reduce a PDF by re-compressing content streams and dropping unused objects. The document stays fully readable.",
                domain: Kind::Pdf,
                input_formats: &["PDF"],
                output_capability: "Compressed PDF document",
                engine: ToolEngine::Builtin,
                batch: false,
                min_files: 1,
                max_files: None,
                extensions: &["pdf"],
            },
            ToolId::ConvertAudio => ToolSpec {
                id: *self,
                title: "Convert audio",
                tagline: "Transcode between audio codecs with custom bitrate",
                description: "Transcode sound files between MP3, WAV, M4A and OGG with bitrates from 64 to 320 kbps.",
                domain: Kind::Audio,
                input_formats: &["MP3", "WAV", "M4A", "OGG"],
                output_capability: "MP3, WAV, M4A or OGG audio file",
                engine: ToolEngine::Ffmpeg,
                batch: false,
                min_files: 1,
                max_files: None,
                extensions: &["mp3", "wav", "m4a", "ogg"],
            },
            ToolId::TrimAudio => ToolSpec {
                id: *self,
                title: "Trim audio",
                tagline: "Cut audio clips with millisecond boundary accuracy",
                description: "Extract an audio segment using exact start and end timestamps (seconds or mm:ss format).",
                domain: Kind::Audio,
                input_formats: &["MP3", "WAV", "M4A", "OGG"],
                output_capability: "Trimmed audio segment",
                engine: ToolEngine::Ffmpeg,
                batch: false,
                min_files: 1,
                max_files: None,
                extensions: &["mp3", "wav", "m4a", "ogg"],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_spec() {
        for tool in ToolId::ALL {
            let spec = tool.spec();
            assert_eq!(spec.id, *tool);
            assert!(!spec.title.is_empty());
            assert!(!spec.description.is_empty());
            assert!(!spec.extensions.is_empty());
            assert!(spec.min_files >= 1);
        }
    }

    #[test]
    fn batch_tools_are_flagged() {
        assert!(ToolId::MergePdf.spec().batch);
        assert!(ToolId::ImagesToPdf.spec().batch);
        assert!(!ToolId::ConvertImage.spec().batch);
        assert_eq!(ToolId::MergePdf.spec().min_files, 2);
    }

    #[test]
    fn external_engines_are_declared() {
        assert_eq!(ToolId::ConvertAudio.spec().engine, ToolEngine::Ffmpeg);
        assert_eq!(ToolId::TrimAudio.spec().engine, ToolEngine::Ffmpeg);
        assert_eq!(ToolId::PdfToImages.spec().engine, ToolEngine::PdfRenderer);
        assert_eq!(ToolId::ConvertImage.spec().engine, ToolEngine::Builtin);
    }
}
