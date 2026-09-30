//! The platform-neutral representation of what travels through the Cross Clipboard.

/// Content of a Cross Clipboard item.
///
/// Text is kept exactly as copied (line endings included). Images are always PNG,
/// whatever format the source platform used on its clipboard.
#[derive(Clone, PartialEq, Eq)]
pub enum ClipContent {
    Text(String),
    Image {
        png: Vec<u8>,
        width: u32,
        height: u32,
    },
}

impl ClipContent {
    /// Short human-readable description used in notifications and logs,
    /// e.g. "3 lines of text" or "image 1920×1080".
    pub fn summary(&self) -> String {
        match self {
            ClipContent::Text(text) => {
                let lines = text.lines().count().max(1);
                if lines == 1 {
                    format!("text ({} chars)", text.chars().count())
                } else {
                    format!("{lines} lines of text")
                }
            }
            ClipContent::Image { width, height, .. } => format!("image {width}×{height}"),
        }
    }

    /// Size of the payload in bytes.
    pub fn len(&self) -> usize {
        match self {
            ClipContent::Text(text) => text.len(),
            ClipContent::Image { png, .. } => png.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::fmt::Debug for ClipContent {
    // Never dump clipboard contents into logs: they can contain secrets.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClipContent({}, {} bytes)", self.summary(), self.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_describes_content() {
        assert_eq!(
            ClipContent::Text("héllo".into()).summary(),
            "text (5 chars)"
        );
        assert_eq!(
            ClipContent::Text("a\nb\r\nc".into()).summary(),
            "3 lines of text"
        );
        assert_eq!(ClipContent::Text(String::new()).summary(), "text (0 chars)");
        let image = ClipContent::Image {
            png: vec![0; 10],
            width: 1920,
            height: 1080,
        };
        assert_eq!(image.summary(), "image 1920×1080");
    }

    #[test]
    fn debug_does_not_leak_content() {
        let debug = format!("{:?}", ClipContent::Text("hunter2".into()));
        assert!(!debug.contains("hunter2"), "{debug}");
    }
}
