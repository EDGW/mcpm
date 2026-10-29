"""Build deterministic metadata-only mods covering dependency match states."""

import io
import json
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).parent / "dependency-problems" / "mods"
KINDS = ("required", "optional", "recommended", "suggested", "breaks", "conflicts")
FABRIC_KEYS = {
    "required": "depends", "recommended": "recommends", "suggested": "suggests",
    "breaks": "breaks", "conflicts": "conflicts",
}
NEO_TYPES = {
    "required": "required", "optional": "optional", "breaks": "incompatible",
    "conflicts": "discouraged",
}


def toml_string(value):
    return json.dumps(value, ensure_ascii=False)


def fml(modid, version, loader, dependencies):
    document = (
        'modLoader="javafml"\nloaderVersion="[1,)"\nlicense="MIT"\n'
        f'[[mods]]\nmodId={toml_string(modid)}\n'
        f'displayName={toml_string(modid)}\nversion={toml_string(version)}\n'
    )
    for kind, target, constraint in dependencies:
        if loader == "forge":
            declaration = f'mandatory={str(kind == "required").lower()}'
        else:
            declaration = f'type={toml_string(NEO_TYPES[kind])}'
        document += (
            f'\n[[dependencies.{modid}]]\nmodId={toml_string(target)}\n'
            f'{declaration}\nversionRange={toml_string(constraint)}\n'
            'side="BOTH"\nordering="NONE"\n'
        )
    return document


def jar(modid, version="1.0.0", dependencies=(), loaders=("fabric", "forge", "neoforge")):
    documents = {}
    for loader in loaders:
        if loader == "fabric":
            document = {"schemaVersion": 1, "id": modid, "name": modid, "version": version}
            for kind, target, constraint in dependencies:
                document.setdefault(FABRIC_KEYS[kind], {})[target] = constraint
            documents["fabric.mod.json"] = json.dumps(document, ensure_ascii=False, indent=2) + "\n"
        else:
            path = "META-INF/mods.toml" if loader == "forge" else "META-INF/neoforge.mods.toml"
            documents[path] = fml(modid, version, loader, dependencies)
    result = io.BytesIO()
    with ZipFile(result, "w") as archive:
        for path, document in sorted(documents.items()):
            entry = ZipInfo(path, date_time=(2020, 1, 1, 0, 0, 0))
            entry.compress_type = ZIP_DEFLATED
            archive.writestr(entry, document.encode("utf-8"))
    (ROOT / f"{modid}.jar").write_bytes(result.getvalue())


def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    for kind in KINDS:
        loaders = tuple(loader for loader, supported in (
            ("fabric", kind in FABRIC_KEYS),
            ("forge", kind in ("required", "optional")),
            ("neoforge", kind in NEO_TYPES),
        ) if supported)
        # Each supported kind gets the same four cases, including a missing target.
        for state in ("matched", "unmatched"):
            jar(f"{kind}_{state}", loaders=loaders)
        for loader in loaders:
            # Separate IDs avoid inventing a common version format for different loaders.
            jar(f"{kind}_unknown_{loader}",
                version="release" if loader == "fabric" else "版本未知", loaders=(loader,))
        # Each root only contains the dependency categories supported by its loader.
        for loader in loaders:
            constraints = ("*", ">=2", ">release", ">=1") if loader == "fabric" else (
                "[1,)", "[2,)", "[2,)", "[1,)"
            )
            targets = (f"{kind}_matched", f"{kind}_unmatched",
                       f"{kind}_unknown_{loader}", f"{kind}_missing")
            jar(f"case_{kind}_{loader}", dependencies=tuple(
                (kind, target, constraint) for target, constraint in zip(targets, constraints)
            ), loaders=(loader,))
    jar("isolated")
    jar("fabric_only", loaders=("fabric",))
    for loader in ("forge", "neoforge"):
        jar(f"case_cross_loader_{loader}",
            dependencies=(("required", "fabric_only", "[1,)"),), loaders=(loader,))


if __name__ == "__main__":
    main()
