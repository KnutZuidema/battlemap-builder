use std::sync::Arc;
use async_trait::async_trait;
use thiserror::Error;

/// Errors arising during vision-based physical description generation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DescriptionError {
    #[error("Vision description engine not configured: {0}")]
    NotConfigured(String),

    #[error("Image data unavailable: {0}")]
    ImageUnavailable(String),

    #[error("Vision description generation failed: {0}")]
    GenerationFailed(String),

    #[error("Empty description received from generator")]
    EmptyDescription,
}

/// Abstract port for generating perspective-neutral physical descriptions of battlemaps.
#[async_trait]
pub trait DescriptionGenerator: Send + Sync {
    /// Generates physical description from image bytes, optional filename, tags, and optional title.
    async fn generate_description(
        &self,
        image_bytes: &[u8],
        filename: Option<&str>,
        tags: &[String],
        title: Option<&str>,
    ) -> Result<String, DescriptionError>;

    /// Convenience helper omitting optional filename and title.
    async fn generate_description_simple(
        &self,
        image_bytes: &[u8],
        tags: &[String],
    ) -> Result<String, DescriptionError> {
        self.generate_description(image_bytes, None, tags, None).await
    }

}

/// Domain service orchestrating physical description generation without silent fallbacks.
#[derive(Clone)]
pub struct DescriptionService {
    generator: Option<Arc<dyn DescriptionGenerator>>,
}

impl DescriptionService {
    /// Constructs a new `DescriptionService` with an optional backing generator.
    pub fn new(generator: Option<Arc<dyn DescriptionGenerator>>) -> Self {
        Self { generator }
    }

    /// Returns `true` if a description generator is configured.
    pub fn is_configured(&self) -> bool {
        self.generator.is_some()
    }

    /// Generates physical description from image bytes, optional filename, tags, and optional title.
    pub async fn describe(
        &self,
        image_bytes: &[u8],
        filename: Option<&str>,
        tags: &[String],
        title: Option<&str>,
    ) -> Result<String, DescriptionError> {
        if image_bytes.is_empty() {
            return Err(DescriptionError::ImageUnavailable(
                "Image byte buffer is empty".to_string(),
            ));
        }

        let gen = self.generator.as_ref().ok_or_else(|| {
            DescriptionError::NotConfigured(
                "Vision description engine is not configured (missing OPENROUTER_API_KEY or disabled)".to_string(),
            )
        })?;

        let desc = gen
            .generate_description(image_bytes, filename, tags, title)
            .await?;

        let trimmed = desc.trim().to_string();
        if trimmed.is_empty() {
            return Err(DescriptionError::EmptyDescription);
        }

        Ok(trimmed)
    }

    /// Generates physical description without extra title/context.
    pub async fn describe_image(
        &self,
        image_bytes: &[u8],
        filename: Option<&str>,
        tags: &[String],
    ) -> Result<String, DescriptionError> {
        self.describe(image_bytes, filename, tags, None).await
    }
}

/// Resolves an image MIME type given a MIME string, filename, or raw magic bytes.
pub fn resolve_mime_type(mime_or_filename: &str, bytes: &[u8]) -> String {
    let trimmed = mime_or_filename.trim();
    if trimmed.contains('/') {
        return trimmed.to_string();
    }
    if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        return "image/png".to_string();
    }
    if bytes.len() >= 3 && &bytes[0..3] == b"\xFF\xD8\xFF" {
        return "image/jpeg".to_string();
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp".to_string();
    }
    if bytes.len() >= 6 && (&bytes[0..6] == b"GIF87a" || &bytes[0..6] == b"GIF89a") {
        return "image/gif".to_string();
    }
    if let Some(ext) = trimmed.rsplit('.').next() {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => return "image/jpeg".to_string(),
            "png" => return "image/png".to_string(),
            "webp" => return "image/webp".to_string(),
            "gif" => return "image/gif".to_string(),
            _ => {}
        }
    }
    "image/png".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct TestGen {
        outcome: Result<String, DescriptionError>,
        invoked: AtomicBool,
    }

    #[async_trait]
    impl DescriptionGenerator for TestGen {
        async fn generate_description(
            &self,
            _image_bytes: &[u8],
            _filename: Option<&str>,
            _tags: &[String],
            _title: Option<&str>,
        ) -> Result<String, DescriptionError> {
            self.invoked.store(true, Ordering::SeqCst);
            self.outcome.clone()
        }
    }

    #[tokio::test]
    async fn test_service_unconfigured() {
        let svc = DescriptionService::new(None);
        assert!(!svc.is_configured());

        let res = svc.describe(b"fake-bytes", Some("image/png"), &[], None).await;
        assert!(matches!(res, Err(DescriptionError::NotConfigured(_))));
    }

    #[tokio::test]
    async fn test_service_empty_bytes() {
        let gen = Arc::new(TestGen {
            outcome: Ok("desc".into()),
            invoked: AtomicBool::new(false),
        });
        let svc = DescriptionService::new(Some(gen));

        let res = svc.describe(b"", Some("image/png"), &[], None).await;
        assert!(matches!(res, Err(DescriptionError::ImageUnavailable(_))));
    }

    #[tokio::test]
    async fn test_service_success_trimmed() {
        let gen = Arc::new(TestGen {
            outcome: Ok("  Evocative stone cavern with moss.  \n".into()),
            invoked: AtomicBool::new(false),
        });
        let svc = DescriptionService::new(Some(gen.clone()));
        assert!(svc.is_configured());

        let res = svc
            .describe(b"fake-image", Some("map.png"), &["cave".into()], Some("underground"))
            .await
            .unwrap();
        assert_eq!(res, "Evocative stone cavern with moss.");
        assert!(gen.invoked.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_service_empty_returned_description() {
        let gen = Arc::new(TestGen {
            outcome: Ok("   \n\t ".into()),
            invoked: AtomicBool::new(false),
        });
        let svc = DescriptionService::new(Some(gen));

        let res = svc.describe(b"fake-image", Some("image/png"), &[], None).await;
        assert_eq!(res, Err(DescriptionError::EmptyDescription));
    }

    #[tokio::test]
    async fn test_resolve_mime_type() {
        assert_eq!(resolve_mime_type("image/jpeg", b""), "image/jpeg");
        assert_eq!(resolve_mime_type("image/webp", b""), "image/webp");
        assert_eq!(resolve_mime_type("foo.jpg", b""), "image/jpeg");
        assert_eq!(resolve_mime_type("foo.png", b""), "image/png");
        assert_eq!(resolve_mime_type("unknown", b"\x89PNG\r\n\x1a\n"), "image/png");
        assert_eq!(resolve_mime_type("unknown", b"\xFF\xD8\xFF"), "image/jpeg");
    }
}
