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


def nested_archive(modid, name, child=None, extra_mods=()):
    fabric = {"schemaVersion": 1, "id": modid, "name": name, "version": "1.0.0"}
    forge = f'''modLoader="javafml"
loaderVersion="[52,)"
license="MIT"
[[mods]]
modId="{modid}"
displayName="{name}"
version="1.0.0"
'''
    neo = forge.replace("[52,)", "[4,)")
    for extra_id, extra_name in extra_mods:
        declaration = f'\n[[mods]]\nmodId="{extra_id}"\ndisplayName="{extra_name}"\nversion="1.0.0"\n'
        forge += declaration
        neo += declaration
    documents = {}
    if child is not None:
        path = "META-INF/jars/grandchild.jar"
        fabric["jars"] = [{"file": path}]
        documents[path] = child
        documents["META-INF/jarjar/metadata.json"] = jarjar(path, "grandchild")
        fabric["depends"] = {"omnibus_leaf": ">=1"}
        forge += '\n[[dependencies.omnibus_nested]]\nmodId="omnibus_leaf"\nmandatory=true\nversionRange="[1,)"\n'
        neo += '\n[[dependencies.omnibus_nested]]\nmodId="omnibus_leaf"\ntype="required"\nversionRange="[1,)"\n'
    documents.update({
        "fabric.mod.json": json_document(fabric),
        "META-INF/mods.toml": forge,
        "META-INF/neoforge.mods.toml": neo,
    })
    return bundle(documents)


grandchild = nested_archive("omnibus_leaf", "Leaf with no dependencies")
child = nested_archive("omnibus_nested", "Nested Module", grandchild)
child_path = "META-INF/jars/child.jar"
extension_path = "META-INF/jars/extension.jar"
library_path = "META-INF/jars/plain-library.jar"
extension = nested_archive("omnibus_extension", "Extension One", extra_mods=[
    ("extension_two", "Extension Two"), ("extension_three", "Extension Three"),
])
library = bundle({"META-INF/MANIFEST.MF": "Manifest-Version: 1.0\r\n\r\n"})
inner_paths = [child_path, extension_path, library_path]

fabric = {
    "schemaVersion": 1, "id": "omnibus", "version": "3.2.1+fabric",
    "name": "Complex Mod / Fabric", "jars": [{"file": path} for path in inner_paths],
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

jarjar_metadata = {"jars": [
    json.loads(jarjar(path, artifact))["jars"][0]
    for path, artifact in zip(inner_paths, ["child", "extension", "plain-library"])
]}

documents = {
    "fabric.mod.json": json_document(fabric),
    "META-INF/mods.toml": forge,
    "META-INF/neoforge.mods.toml": neo,
    "META-INF/jarjar/metadata.json": json_document(jarjar_metadata),
    "META-INF/MANIFEST.MF": "Manifest-Version: 1.0\r\nImplementation-Version: 3.2.1\r\n\r\n",
    child_path: child,
    extension_path: extension,
    library_path: library,
}
Path(__file__).with_name("complex-mod.jar").write_bytes(bundle(documents))
