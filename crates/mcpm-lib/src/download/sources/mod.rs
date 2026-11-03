//! Built-in and configurable URL mappings; concrete sources are assembled here.

use std::sync::Arc;

use anyhow::{Result, bail};
use url::Url;

use super::{DownloadConfig, DownloadSource, Downloader};

mod mapped;
pub use mapped::{MappedSource, UrlMapping};

/// Builds the configured built-in sources in exactly the requested order.
/// Unknown source names are configuration errors; no fallback is appended.
pub fn downloader(config: &DownloadConfig) -> Result<Downloader> {
    let sources = config
        .sources()
        .iter()
        .map(|id| -> Result<Arc<dyn DownloadSource>> {
            match id.as_str() {
                "official" => Ok(Arc::new(OfficialSource)),
                "bmclapi" => Ok(Arc::new(bmclapi()?)),
                _ => bail!("unknown built-in download source {id:?}"),
            }
        })
        .collect::<Result<_>>()?;
    Downloader::new(sources, config.options())
}

/// Uses the original HTTP(S) address without translation.
#[derive(Debug, Clone, Copy, Default)]
pub struct OfficialSource;

impl DownloadSource for OfficialSource {
    fn id(&self) -> &str {
        "official"
    }

    fn translate(&self, original: &Url) -> Result<Vec<Url>> {
        Ok(vec![original.clone()])
    }
}

/// BMCLAPI's protocol-compatible mirrors. Different-format catalog APIs are
/// deliberately excluded. Maven paths shared by multiple upstreams cannot be reversed.
pub fn bmclapi() -> Result<MappedSource> {
    let root = "https://bmclapi2.bangbang93.com";
    let mappings = [
        ("https://launchermeta.mojang.com/", "/"),
        ("https://piston-meta.mojang.com/", "/"),
        ("https://piston-data.mojang.com/", "/"),
        ("https://launcher.mojang.com/", "/"),
        ("https://resources.download.minecraft.net/", "/assets/"),
        ("https://libraries.minecraft.net/", "/libraries/"),
        ("https://maven.minecraftforge.net/", "/maven/"),
        ("https://files.minecraftforge.net/maven/", "/maven/"),
        ("https://maven.neoforged.net/releases/", "/maven/"),
        ("https://meta.fabricmc.net/", "/fabric-meta/"),
        ("https://maven.fabricmc.net/", "/maven/"),
        (
            "https://authlib-injector.yushi.moe/",
            "/mirrors/authlib-injector/",
        ),
    ]
    .into_iter()
    .map(|(original, path)| {
        UrlMapping::new(Url::parse(original)?, Url::parse(&format!("{root}{path}"))?)
    })
    .collect::<Result<_>>()?;
    MappedSource::new("bmclapi", mappings)
}
