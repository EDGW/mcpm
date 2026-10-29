"""Deterministic metadata-only JARs and static scripts; none are runnable servers.

Empty .class entries are identity markers, not bytecode. These fixtures test metadata
inspection and refusal rules, not authenticity or launchability.
"""
import io
import json
from pathlib import Path
from zipfile import ZIP_STORED, ZipFile, ZipInfo

ROOT = Path(__file__).parent / "server-init"


def write(case, path, data):
    target = ROOT / case / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data if isinstance(data, bytes) else data.encode())


def jar(entries):
    output = io.BytesIO()
    with ZipFile(output, "w", ZIP_STORED) as archive:
        for name, value in sorted(entries.items()):
            info = ZipInfo(name, (2020, 1, 1, 0, 0, 0))
            info.external_attr = 0o100644 << 16
            archive.writestr(info, value)
    return output.getvalue()


def manifest(main, cp=None):
    return "Manifest-Version: 1.0\r\nMain-Class: " + main + "\r\n" + (
        "Class-Path: " + cp + "\r\n" if cp else ""
    ) + "\r\n"


def vanilla(case, version="1.20.1", name="server.jar"):
    write(case, name, jar({
        "META-INF/MANIFEST.MF": manifest("net.minecraft.server.Main"),
        "net/minecraft/server/Main.class": b"",
        "version.json": json.dumps({"id": version}),
    }))


BOOT = "libraries/cpw/mods/bootstraplauncher/1.1.2/bootstraplauncher-1.1.2.jar"
GAME = "libraries/net/minecraft/server/1.20.1/server-1.20.1-extra.jar"
FORGE = "libraries/net/minecraftforge/fmlloader/1.20.1-47.3.12/fmlloader-1.20.1-47.3.12.jar"
NEO = "libraries/net/neoforged/neoforge/21.1.77/neoforge-21.1.77-universal.jar"
EARLY = "libraries/net/neoforged/forge/1.20.1-47.1.79/forge-1.20.1-47.1.79-universal.jar"


def fml(case, kind="forge", bad_game=False):
    loader = {"forge": FORGE, "neoforge": NEO, "early-neoforge": EARLY}[kind]
    marker = {"forge": "net/minecraftforge/fml/loading/FMLLoader.class",
              "neoforge": "net/neoforged/neoforge/common/NeoForgeMod.class",
              "early-neoforge": "net/minecraftforge/common/ForgeMod.class"}[kind]
    version = {"forge": "47.3.12", "neoforge": "21.1.77", "early-neoforge": "47.1.79"}[kind]
    mc = "1.21.1" if kind == "neoforge" else "1.20.1"
    write(case, BOOT, jar({"cpw/mods/bootstraplauncher/BootstrapLauncher.class": b""}))
    write(case, loader, jar({marker: b""}))
    write(case, GAME, jar({"version.json": json.dumps({"id": "1.19.4" if bad_game else mc})}))
    argpath = "launch-args.txt"
    group = "net.minecraftforge" if kind == "forge" else "net.neoforged"
    option = "--fml.neoForgeVersion" if kind == "neoforge" else "--fml.forgeVersion"
    args = f"-p {BOOT}\n-DlegacyClassPath={loader}:{GAME}\ncpw.mods.bootstraplauncher.BootstrapLauncher\n--launchTarget forgeserver\n{option} {version}\n--fml.mcVersion {mc}\n--fml.forgeGroup {group}\n"
    write(case, argpath, args)
    write(case, "user_jvm_args.txt", "# JVM tuning\n-Xmx2G\n-XX:+UseG1GC\n")
    write(case, "run.sh", 'java @user_jvm_args.txt @launch-args.txt nogui "$@"\n')
    write(case, "launch-args-win.txt", args.replace(f"{loader}:{GAME}", f"{loader};{GAME}"))
    write(case, "run.bat", '@echo off\r\njava @user_jvm_args.txt @launch-args-win.txt nogui %*\r\npause\r\n')


fml("forge")
fml("neoforge", "neoforge")
fml("early-neoforge", "early-neoforge")
fml("game-version-conflict", bad_game=True)
fml("loader-version-conflict")
p = ROOT / "loader-version-conflict/launch-args.txt"
p.write_text(p.read_text().replace("--fml.forgeVersion 47.3.12", "--fml.forgeVersion 47.4.0"))
fml("missing-loader")
(ROOT / "missing-loader" / FORGE).unlink()
fml("fake-loader")
write("fake-loader", FORGE, jar({"unrelated.txt": "not a loader"}))
fml("duplicate-version")
with (ROOT / "duplicate-version/launch-args.txt").open("a") as file:
    file.write("--fml.mcVersion 1.19.4\n")
fml("client-target")
p = ROOT / "client-target/launch-args.txt"
p.write_text(p.read_text().replace("forgeserver", "forgeclient"))

for case in ["vanilla", "nested-shell", "nested-batch", "multiple", "partial-unknown", "dynamic-control", "dynamic-command", "undefined-variable", "unquoted-variable", "cycle", "argument-cycle", "unknown-jvm-options"]:
    vanilla(case)
    write(case, "run.sh", 'java -jar server.jar nogui "$@"\n')
    write(case, "run.bat", '@echo off\r\njava -jar server.jar nogui %*\r\npause\r\n')

write("nested-shell", "run.sh", 'ROOT="$(dirname "$0")"\ncd "$ROOT"\nsource ./vars.sh\n. ./scripts/start.sh\n')
write("nested-shell", "vars.sh", 'JVM="-Xmx2G"\n')
write("nested-shell", "scripts/start.sh", 'exec java "$JVM" -jar server.jar nogui "$@"\n')
write("nested-batch", "run.bat", '@echo off\r\nsetlocal\r\ncd /d "%~dp0"\r\nset "JVM=-Xmx2G"\r\ncall scripts\\start.bat\r\nendlocal\r\n')
write("nested-batch", "scripts/start.bat", 'java %JVM% -jar server.jar nogui %*\r\n')
vanilla("multiple", "1.21.1", "other.jar")
write("multiple", "run.sh", 'java -jar server.jar\njava -jar other.jar\n')
write("multiple", "run.bat", '@echo off\r\njava -jar server.jar\r\njava -jar other.jar\r\n')
write("partial-unknown", "run.sh", 'java -jar server.jar\njava com.example.UnknownServer\n')
write("dynamic-control", "run.sh", 'if test -f server.jar; then\njava -jar server.jar\nfi\n')
write("dynamic-command", "run.sh", 'JAVA=$(command -v java)\n"$JAVA" -jar server.jar\n')
write("undefined-variable", "run.sh", 'java $JAVA_ARGS -jar server.jar\n')
write("unquoted-variable", "run.sh", 'JAVA_ARGS="-Xmx2G -Xms1G"\njava $JAVA_ARGS -jar server.jar\n')
write("cycle", "run.sh", 'source ./child.sh\n')
write("cycle", "child.sh", 'source ./run.sh\n')
write("argument-cycle", "run.sh", 'java @a.txt\n')
write("argument-cycle", "a.txt", '@b.txt\n')
write("argument-cycle", "b.txt", '@a.txt\n')
write("unknown-jvm-options", "run.sh", 'java --unknown-launcher-option --future-option=value -XX:FutureTuning=42 -jar server.jar\n')
write("unknown-main", "run.sh", 'java com.example.UnknownServer\n')
write("missing-jar", "run.sh", 'java -jar absent.jar\n')
write("corrupt-jar", "run.sh", 'java -jar server.jar\n')
write("corrupt-jar", "server.jar", b"not a zip archive\n")
write("fake-vanilla", "run.sh", 'java -jar server.jar\n')
write("fake-vanilla", "server.jar", jar({"META-INF/MANIFEST.MF": manifest("net.minecraft.server.Main"), "version.json": '{"id":"1.20.1"}'}))
write("no-java", "run.sh", '#!/bin/sh\necho "No launch here"\n')
write("dynamic-batch", "run.bat", '@echo off\r\nif exist server.jar java -jar server.jar\r\n')
write("delayed-batch", "run.bat", '@echo off\r\njava !JAVA_ARGS! -jar server.jar\r\n')

# Fabric full installer output, with manifest continuation and renamed launch JAR.
LOADER = "libraries/net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar"
INTERMEDIARY = "libraries/net/fabricmc/intermediary/1.20.1/intermediary-1.20.1.jar"
for case in ["fabric", "fabric-version-conflict"]:
    vanilla(case)
    write(case, LOADER, jar({"fabric.mod.json": '{"id":"fabricloader","version":"0.16.10"}', "net/fabricmc/loader/impl/launch/server/FabricServerLauncher.class": b""}))
    write(case, INTERMEDIARY, jar({"mappings/mappings.tiny": "tiny\t2\t0\tofficial\tintermediary\n"}))
    write(case, "renamed.jar", jar({
        "META-INF/MANIFEST.MF": manifest("net.fabricmc.loader.impl.launch.server.FabricServerLauncher", LOADER + "\r\n  " + INTERMEDIARY),
        "fabric-server-launch.properties": "launch.mainClass=net.fabricmc.loader.impl.launch.knot.KnotServer\n",
    }))
    write(case, "run.sh", 'java -jar renamed.jar nogui\n')
vanilla("fabric-version-conflict", "1.19.4")

for case in ["fabric-auto", "fabric-auto-missing-version"]:
    props = "fabric-loader-version=0.16.10\ngame-version=1.20.1\n"
    if case.endswith("missing-version"):
        props = "game-version=1.20.1\n"
    write(case, "server.jar", jar({"META-INF/MANIFEST.MF": manifest("net.fabricmc.installer.ServerLauncher"), "net/fabricmc/installer/ServerLauncher.class": b"", "install.properties": props}))
    write(case, "run.sh", "java -jar server.jar nogui\n")

# Newer Forge shim: versions come from referenced artifacts, cross-checked with game JSON.
case = "forge-shim"
universal = "net/minecraftforge/forge/1.21.1-52.0.1/forge-1.21.1-52.0.1-universal.jar"
game = "net/minecraft/server/1.21.1/server-1.21.1-extra.jar"
write(case, "libraries/" + universal, jar({"net/minecraftforge/common/ForgeMod.class": b""}))
write(case, "libraries/" + game, jar({"version.json": '{"id":"1.21.1"}'}))
write(case, "shim.jar", jar({
    "META-INF/MANIFEST.MF": manifest("net.minecraftforge.bootstrap.shim.Main"),
    "net/minecraftforge/bootstrap/shim/Main.class": b"",
    "bootstrap-shim.properties": "Main-Class=net.minecraftforge.bootstrap.ForgeBootstrap\nArguments=--launchTarget forge_server\n",
    "bootstrap-shim.list": "\n".join("0" * 64 + "\tartifact\t" + path for path in [universal, game]) + "\n",
}))
write(case, "run.sh", "java -jar shim.jar nogui\n")

# Bundled vanilla validates the version list against the embedded JAR's version.json.
case = "bundled-vanilla"
inner = jar({"version.json": '{"id":"1.21.1"}'})
write(case, "server.jar", jar({"META-INF/MANIFEST.MF": manifest("net.minecraft.bundler.Main"), "net/minecraft/bundler/Main.class": b"", "META-INF/versions.list": "hash\t1.21.1\tserver.jar\n", "META-INF/versions/server.jar": inner}))
write(case, "run.sh", "java -jar server.jar nogui\n")

print("Built static server identification fixtures in", ROOT)
