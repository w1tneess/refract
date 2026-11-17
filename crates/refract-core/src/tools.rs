use crate::format::Kind;
use serde::{Deserialize, Serialize};

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
}
