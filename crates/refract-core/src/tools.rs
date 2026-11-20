use crate::format::Kind;
pub use crate::generated_manifest::{
    limits_for_key, manifest_for, LimitsSpec, ManifestOptionSpec, OptionChoice, OptionDefault,
    OptionShowIf, OptionType, Platform, PlatformAvailability, ToolEngine, ToolId,
    ToolManifestEntry, FORMAT_SUPPORT, LIMITS_MANIFEST, MANIFEST_VERSION, TOOL_MANIFEST,
};

/// Static description of a tool: how it is presented and what it needs on Desktop.
#[derive(Debug, Clone, Copy)]
pub struct ToolSpec {
    pub id: ToolId,
    pub title: &'static str,
    pub tagline: &'static str,
    pub description: &'static str,
    pub domain: Kind,
    /// Labels of the accepted desktop input formats.
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
    /// File extensions accepted by the desktop file picker (without dot).
    pub extensions: &'static [&'static str],
}

impl ToolSpec {
    pub fn accepts_multiple(&self) -> bool {
        self.max_files.map(|m| m > 1).unwrap_or(self.batch)
    }
}

impl ToolId {
    pub fn manifest(&self) -> &'static ToolManifestEntry {
        manifest_for(*self)
    }

    pub fn label(&self) -> &'static str {
        self.manifest().label
    }

    pub fn accepts(&self) -> Kind {
        self.manifest().accepts_kind
    }

    pub fn limits(&self) -> &'static LimitsSpec {
        limits_for_key(self.manifest().limits_key)
            .expect("Every tool manifest entry references a valid limits_key")
    }

    pub fn options_for_platform(&self, platform: Platform) -> Vec<&'static ManifestOptionSpec> {
        // Cache platform views to preserve the public static-reference API.
        static OPTIONS: std::sync::OnceLock<Vec<(ToolId, Platform, ManifestOptionSpec)>> =
            std::sync::OnceLock::new();
        OPTIONS
            .get_or_init(|| {
                let mut views = Vec::new();
                for entry in TOOL_MANIFEST {
                    for platform in [Platform::Web, Platform::Desktop] {
                        for option in entry
                            .options
                            .iter()
                            .filter(|o| o.platforms.contains(&platform))
                        {
                            let mut option = *option;
                            option.choices = match platform {
                                Platform::Web => option.web_choices,
                                Platform::Desktop => option.desktop_choices,
                            };
                            views.push((entry.id, platform, option));
                        }
                    }
                }
                views
            })
            .iter()
            .filter(|(id, p, _)| id == self && *p == platform)
            .map(|(_, _, option)| option)
            .collect()
    }

    /// Full presentation + capability metadata for the tool, derived from the canonical manifest.
    pub fn spec(&self) -> ToolSpec {
        let entry = self.manifest();
        ToolSpec {
            id: entry.id,
            title: entry.label,
            tagline: entry.tagline,
            description: entry.description,
            domain: entry.domain,
            input_formats: entry.desktop_input_format_labels,
            output_capability: entry.output_capability,
            engine: entry.desktop_engine,
            batch: entry.batch,
            min_files: entry.min_files,
            max_files: entry.max_files,
            extensions: entry.desktop_extensions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use serde_json::Value;
    use std::collections::HashSet;

    const RAW_MANIFEST: &str = include_str!("../../../specs/tool-manifest.json");

    #[test]
    fn every_tool_has_a_spec() {
        assert_eq!(ToolId::ALL.len(), 11);
        for tool in ToolId::ALL {
            let spec = tool.spec();
            assert_eq!(spec.id, *tool);
            assert!(!spec.title.is_empty());
            assert!(!spec.description.is_empty());
            assert!(!spec.extensions.is_empty());
            assert!(spec.min_files >= 1);
            assert_eq!(ToolId::from_id_str(tool.id_str()), Some(*tool));
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
        assert_eq!(
            ToolId::ConvertAudio.manifest().desktop_availability,
            PlatformAvailability::RequiresDependency
        );
        assert_eq!(
            ToolId::PdfToImages.manifest().desktop_availability,
            PlatformAvailability::RequiresDependency
        );
    }

    #[test]
    fn platform_differences_are_explicit() {
        // HEIC is web-only decode; desktop must not claim HEIC support.
        let heic_support = FORMAT_SUPPORT
            .iter()
            .find(|f| f.format == Format::Heic)
            .expect("HEIC format entry exists");
        assert!(heic_support.web_decode);
        assert!(!heic_support.web_encode);
        assert!(!heic_support.desktop_decode);
        assert!(!heic_support.desktop_encode);

        for tool in [
            ToolId::ConvertImage,
            ToolId::CompressImage,
            ToolId::ResizeImage,
            ToolId::ImagesToPdf,
        ] {
            let m = tool.manifest();
            assert!(m.web_accepted_formats.contains(&Format::Heic));
            assert!(!m.desktop_accepted_formats.contains(&Format::Heic));
        }

        // Images-to-PDF preserves web-only pageSize and desktop-only dpi/quality options.
        let web_opts: Vec<&str> = ToolId::ImagesToPdf
            .options_for_platform(Platform::Web)
            .iter()
            .map(|o| o.key)
            .collect();
        let desktop_opts: Vec<&str> = ToolId::ImagesToPdf
            .options_for_platform(Platform::Desktop)
            .iter()
            .map(|o| o.key)
            .collect();
        assert_eq!(web_opts, vec!["pageSize"]);
        assert_eq!(desktop_opts, vec!["dpi", "quality"]);
    }

    #[test]
    fn rust_generated_manifest_matches_canonical_json() {
        let json: Value = serde_json::from_str(RAW_MANIFEST).expect("valid JSON manifest");
        assert_eq!(
            json["manifestVersion"].as_u64().unwrap() as u32,
            MANIFEST_VERSION
        );

        let tools = json["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), TOOL_MANIFEST.len());

        let mut seen = HashSet::new();
        for (idx, raw) in tools.iter().enumerate() {
            let id_str = raw["id"].as_str().unwrap();
            assert!(seen.insert(id_str), "duplicate tool id {}", id_str);

            let entry = &TOOL_MANIFEST[idx];
            assert_eq!(entry.id.id_str(), id_str);
            assert_eq!(entry.label, raw["label"].as_str().unwrap());
            assert_eq!(entry.blurb, raw["blurb"].as_str().unwrap());
            assert_eq!(
                entry.min_files,
                raw["inputs"]["min"].as_u64().unwrap() as usize
            );
            assert_eq!(
                entry.output_template,
                raw["outputNaming"]["template"].as_str().unwrap()
            );
            assert_eq!(
                entry.output_multi,
                raw["outputNaming"]["multi"].as_bool().unwrap()
            );
            assert_eq!(entry.limits_key, raw["limitsKey"].as_str().unwrap());

            let raw_opts = raw["options"].as_array().unwrap();
            assert_eq!(entry.options.len(), raw_opts.len());
            for (opt_idx, raw_opt) in raw_opts.iter().enumerate() {
                let gen_opt = &entry.options[opt_idx];
                assert_eq!(gen_opt.key, raw_opt["key"].as_str().unwrap());
                assert_eq!(gen_opt.label, raw_opt["label"].as_str().unwrap());
                match gen_opt.default {
                    OptionDefault::Number(n) => {
                        assert!((n - raw_opt["default"].as_f64().unwrap()).abs() < f64::EPSILON);
                    }
                    OptionDefault::Text(s) => {
                        assert_eq!(s, raw_opt["default"].as_str().unwrap());
                    }
                }
            }

            // Limits referenced by tool must match JSON values
            let lim = entry.id.limits();
            let raw_lim = &json["limits"][entry.limits_key];
            assert_eq!(lim.web_max_mb, raw_lim["web"]["maxMB"].as_u64().unwrap());
            assert_eq!(
                lim.desktop_max_mb,
                raw_lim["desktop"]["maxMB"].as_u64().unwrap()
            );
            assert_eq!(
                lim.web_max_files,
                raw_lim["web"]["maxFiles"].as_u64().unwrap() as usize
            );
            assert_eq!(
                lim.desktop_max_files,
                raw_lim["desktop"]["maxFiles"].as_u64().unwrap() as usize
            );
        }
    }
    #[test]
    fn unrestricted_choices_remain_on_both_platforms() {
        let desktop = ToolId::ConvertImage.options_for_platform(Platform::Desktop);
        let web = ToolId::ConvertImage.options_for_platform(Platform::Web);
        assert!(desktop
            .iter()
            .find(|o| o.key == "to")
            .unwrap()
            .choices
            .iter()
            .any(|c| c.value == "webp"));
        assert!(web
            .iter()
            .find(|o| o.key == "to")
            .unwrap()
            .choices
            .iter()
            .any(|c| c.value == "webp"));
    }
}
