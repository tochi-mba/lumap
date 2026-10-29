//! Live inputs: textures shared by another application on the same machine. Each platform has
//! its own protocol (Spout on Windows, Syphon on macOS), so this is the one place that knows
//! which exist, which this build carries, and how to open one. Nothing outside this module
//! mentions a platform.

use crate::{FrameSource, SourceError};

/// A live-input protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    /// Spout 2 (Windows, DirectX shared textures).
    Spout,
    /// Syphon (macOS, `IOSurface`).
    Syphon,
}

impl Provider {
    /// Every provider lumap knows of.
    pub const ALL: [Self; 2] = [Self::Spout, Self::Syphon];

    /// The name used in project files and messages.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Spout => "spout",
            Self::Syphon => "syphon",
        }
    }

    /// The platform the protocol belongs to.
    #[must_use]
    pub fn platform(self) -> &'static str {
        match self {
            Self::Spout => "Windows",
            Self::Syphon => "macOS",
        }
    }

    /// Whether this build can open the provider. Spout is compiled in with the `spout` feature
    /// (native on Windows, a never-connected stand-in elsewhere so a show file still opens).
    #[must_use]
    pub fn available(self) -> bool {
        match self {
            Self::Spout => cfg!(feature = "spout"),
            Self::Syphon => false,
        }
    }
}

/// The providers this build can open.
#[must_use]
pub fn available() -> Vec<Provider> {
    Provider::ALL
        .into_iter()
        .filter(|p| p.available())
        .collect()
}

/// Opens a live input, or says why it cannot be opened here. The error names the platform the
/// protocol belongs to, so a project written on a PC reads sensibly on a Mac and back.
pub fn open(
    id: &str,
    provider: Provider,
    sender: &str,
) -> Result<Box<dyn FrameSource>, SourceError> {
    match provider {
        #[cfg(feature = "spout")]
        Provider::Spout => Ok(Box::new(crate::spout::SpoutReceiver::new(sender))),
        _ => {
            let _ = sender;
            Err(SourceError::Unavailable(id.to_string(), provider.name()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_name_their_platform_and_only_the_native_one_is_available() {
        assert_eq!(Provider::Spout.name(), "spout");
        assert_eq!(Provider::Syphon.platform(), "macOS");
        assert!(!Provider::Syphon.available());
        assert_eq!(Provider::Spout.available(), cfg!(feature = "spout"));
        let here = available();
        assert!(here.iter().all(|p| p.available()));
        assert!(!here.contains(&Provider::Syphon));
        let Err(err) = open("cam", Provider::Syphon, "Server") else {
            panic!("Syphon cannot open here")
        };
        assert!(matches!(err, SourceError::Unavailable(ref id, "syphon") if id == "cam"));
        if Provider::Spout.available() {
            assert!(open("tex", Provider::Spout, "Resolume").is_ok());
        } else {
            assert!(open("tex", Provider::Spout, "Resolume").is_err());
        }
    }
}
