"""Build a deterministic, metadata-only JAR for CLI output verification."""

import io
import json
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo


def bundle(documents):
    result = io.BytesIO()
    with ZipFile(result, "w") as archive:
        for name, contents in sorted(documents.items()):
            entry = ZipInfo(name, date_time=(2020, 1, 1, 0, 0, 0))
            entry.compress_type = ZIP_DEFLATED
            archive.writestr(entry, contents)
    return result.getvalue()


def json_document(value):
    return json.dumps(value, ensure_ascii=False, indent=2) + "\n"


def jarjar(path, artifact):
    return json_document({"jars": [{
        "identifier": {"group": "example.fixture", "artifact": artifact},
        "version": {"range": "[1,2)", "artifactVersion": "1.0.0"},
        "path": path, "isObfuscated": False,
    }]})


def nested_archive(modid, name, child=None):
    fabric = {"schemaVersion": 1, "id": modid, "name": name, "version": "1.0.0"}
    quilt = {"schema_version": 1, "quilt_loader": {
        "group": "example.fixture", "id": modid, "version": "1.0.0",
        "metadata": {"name": name},
    }}
    forge = f'''modLoader="javafml"
loaderVersion="[52,)"
license="MIT"
[[mods]]
modId="{modid}"
displayName="{name}"
version="1.0.0"
'''
    neo = forge.replace("[52,)", "[4,)")
    documents = {}
    if child is not None:
        path = "META-INF/jars/grandchild.jar"
        fabric["jars"] = [{"file": path}]
        quilt["quilt_loader"]["jars"] = [path]
        documents[path] = child
        documents["META-INF/jarjar/metadata.json"] = jarjar(path, "grandchild")
        fabric["depends"] = {"omnibus_leaf": ">=1"}
        quilt["quilt_loader"]["depends"] = ["omnibus_leaf"]
        forge += '\n[[dependencies.omnibus_nested]]\nmodId="omnibus_leaf"\nmandatory=true\nversionRange="[1,)"\n'
        neo += '\n[[dependencies.omnibus_nested]]\nmodId="omnibus_leaf"\ntype="required"\nversionRange="[1,)"\n'
    documents.update({
        "fabric.mod.json": json_document(fabric),
        "quilt.mod.json": json_document(quilt),
        "META-INF/mods.toml": forge,
        "META-INF/neoforge.mods.toml": neo,
    })
    return bundle(documents)


grandchild = nested_archive("omnibus_leaf", "Leaf with no dependencies")
child = nested_archive("omnibus_nested", "Nested Module", grandchild)
child_path = "META-INF/jars/child.jar"

fabric = {
    "schemaVersion": 1, "id": "omnibus", "version": "3.2.1+fabric",
    "name": "Complex Mod / Fabric", "jars": [{"file": child_path}],
    "depends": {
        "minecraft": "1.21.x", "java": ">=21", "fabricloader": ">=0.16.0",
        "api": [">=1.0 <2.0", "^3.0"], "tilde_api": "~1.4",
        "exact_api": "=2.0", "not_equal_api": "!=1.5",
        "bounded_api": ">1.0 <=2.0", "hyphen_api": "1.0 - 2.0",
    },
    "recommends": {"recommended_api": "^2.0"},
    "suggests": {"suggested_api": "*"},
    "breaks": {"broken_api": "<1.0"},
    "conflicts": {"conflicting_api": ["1.0", "2.0"]},
}

quilt = {"schema_version": 1, "quilt_loader": {
    "group": "example.fixture", "id": "omnibus", "version": "3.2.1+quilt",
    "metadata": {"name": "Complex Mod / Quilt"}, "jars": [child_path],
    "depends": [
        "quilt_loader",
        {"id": "minecraft", "versions": "1.21.x"},
        [
            {"id": "example:render_api", "versions": {
                "any": [{"all": [">=1", "<2"]}, "^3"]},
             "optional": True, "environment": "client", "reason": "client integration",
             "unless": [{"id": "fallback", "versions": "~2"}, "standalone"]},
            "alternate_api",
        ],
        {"id": "server_api", "versions": "2.0", "environment": "server"},
        {"id": "universal_api", "versions": "*", "environment": "*",
         "reason": "line one\nline two\tend"},
    ],
    "breaks": [
        [{"id": "legacy_api", "versions": "<1"},
         {"id": "legacy_bridge", "versions": "*", "unless": {
             "id": "compatibility", "versions": ">=2", "unless": "compat_disabled"}}],
        {"id": "broken_quilt_api", "versions": "=0.1", "reason": "incompatible API"},
    ],
}}

forge = '''modLoader="javafml"
loaderVersion="[52,)"
license="MIT"
[properties]
release="3.2.1"
[[mods]]
modId="omnibus"
displayName="Complex Mod / Forge"
version="${file.release}+forge"
[[mods]]
modId="forge_tools"
displayName="Forge Tools"
version="${file.jarVersion}"
[[dependencies.omnibus]]
modId="forge"
mandatory=true
versionRange="[52,)"
[[dependencies.omnibus]]
modId="minecraft"
mandatory=true
versionRange="[1.21,1.22)"
side="BOTH"
ordering="NONE"
[[dependencies.omnibus]]
modId="client_api"
mandatory=false
versionRange="(,1.0],[2.0,3.0)"
side="CLIENT"
ordering="AFTER"
reason="optional client integration"
[[dependencies.omnibus]]
modId="server_api"
mandatory=true
versionRange="[2.0]"
side="SERVER"
ordering="BEFORE"
[[dependencies.forge_tools]]
modId="preferred_api"
mandatory=false
versionRange="2.0"
'''

neo = '''modLoader="javafml"
loaderVersion="[4,)"
license="MIT"
[[mods]]
modId="omnibus"
displayName="Complex Mod / NeoForge"
version="${file.jarVersion}"
[[mods]]
modId="neo_tools"
displayName="NeoForge Tools"
version="1.0.0"
[[dependencies.omnibus]]
modId="neoforge"
type="REQUIRED"
versionRange="[21.1,)"
[[dependencies.omnibus]]
modId="minecraft"
versionRange="[1.21.1,1.22)"
[[dependencies.omnibus]]
modId="client_api"
type="Optional"
versionRange="(,1.0],[2.0,)"
side="CLIENT"
ordering="AFTER"
reason="optional API"
[[dependencies.omnibus]]
modId="broken_api"
type="INCOMPATIBLE"
versionRange="[0,1)"
side="SERVER"
reason="broken server integration"
[[dependencies.omnibus]]
modId="conflicting_api"
type="discouraged"
versionRange="[2.0]"
ordering="BEFORE"
reason="known conflict"
'''

documents = {
    "fabric.mod.json": json_document(fabric),
    "quilt.mod.json": json_document(quilt),
    "META-INF/mods.toml": forge,
    "META-INF/neoforge.mods.toml": neo,
    "META-INF/jarjar/metadata.json": jarjar(child_path, "child"),
    "META-INF/MANIFEST.MF": "Manifest-Version: 1.0\r\nImplementation-Version: 3.2.1\r\n\r\n",
    child_path: child,
}
Path(__file__).with_name("complex-mod.jar").write_bytes(bundle(documents))
