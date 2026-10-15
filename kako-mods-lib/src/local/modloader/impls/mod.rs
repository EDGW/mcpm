//! Built-in mod loaders and their shared archive/metadata readers.
//!
//! Nested JARs are read only from declarations in the loader's metadata.
//! Recognition enumerates bundled declarations, without dependency negotiation
//! or deciding which mods a game would load. Ordinary libraries contribute no
//! mod records unless they contain recognized metadata (including Loom-generated
//! Fabric metadata). A child error fails the supplying loader's entire attempt.
//!
//! Sources of truth:
//! - [Forge JarJar](https://docs.minecraftforge.net/en/fg-5.x/dependencies/jarinjar/)
//! - [NeoForge JarJar](https://docs.neoforged.net/toolchain/docs/dependencies/jarinjar/)
//! - [JarJar JSON serializers](https://github.com/MinecraftForge/JarJar/tree/main/metadata/src/main/java/net/minecraftforge/jarjar/metadata/json)
//! - [Fabric metadata](https://docs.fabricmc.net/develop/loader/fabric-mod-json)
//! - [Fabric nested JARs](https://docs.fabricmc.net/develop/loader/#nested-jars)

mod archive;
mod client;
pub use client::ClientLibraryError;
mod constraints;
mod fml;
mod jarjar;
mod metadata;
mod version;

pub mod fabric;
pub mod forge;
pub mod neoforge;
