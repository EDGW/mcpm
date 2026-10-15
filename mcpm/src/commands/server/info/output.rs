use std::io::{self, Write};

use kako_mods_lib::local::mc::server::MinecraftServer;

use crate::{
    commands::output_style::{OutputStyle, text},
    terminal,
};

pub(super) fn write_info(output: &mut impl Write, server: &MinecraftServer) -> io::Result<()> {
    let metadata = server.metadata();
    let loader = server.loader();
    let style = OutputStyle::new();
    let name = text(server.name());
    let path = text(&server.path().display().to_string());
    let version = text(&metadata.minecraft_version);
    let (loader_name, loader_version) = match &loader {
        Some(info) => (text(info.loader.name()), text(&info.version)),
        None => ("vanilla or unrecognized".into(), String::new()),
    };
    if terminal::is_raw() {
        return writeln!(
            output,
            "{name}\t{path}\t{version}\t{loader_name}\t{loader_version}"
        );
    }
    writeln!(
        output,
        "{} {}",
        style.theme("Server:"),
        style.important(name)
    )?;
    writeln!(
        output,
        "{}{}",
        style.secondary("Path: "),
        style.secondary(path)
    )?;
    writeln!(
        output,
        "{}{}",
        style.secondary("Minecraft: "),
        style.important(version)
    )?;
    let loader_name = match &loader {
        Some(info) => style.loader(info.loader.id().as_str(), &loader_name),
        None => style.secondary(loader_name),
    };
    write!(output, "{}{}", style.secondary("Loader: "), loader_name)?;
    if loader.is_some() {
        write!(output, " {}", style.important(loader_version))?;
    }
    writeln!(output)
}
