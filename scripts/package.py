"""Package a tested native release binary without external archive utilities."""
import hashlib
import os
import pathlib
import shutil
import tarfile
import tempfile
import tomllib
import zipfile

root = pathlib.Path(__file__).resolve().parents[1]
version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
arch = os.environ["BUILD_ARCH"]
target = os.environ["BUILD_TARGET"]
binary = os.environ["BUILD_BINARY"]
source = root / "target" / target / "release" / binary
name = f"tensuns-v{version}-{arch}"
out = root / "dist"
out.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory() as work:
    folder = pathlib.Path(work) / name
    folder.mkdir()
    shutil.copy2(source, folder / binary)
    shutil.copy2(root / "README.md", folder / "README.md")
    shutil.copy2(root / "Cargo.lock", folder / "Cargo.lock")
    shutil.copytree(root / "examples", folder / "examples")
    if arch.startswith("windows-"):
        artifact = out / (name + ".zip")
        with zipfile.ZipFile(artifact, "w", zipfile.ZIP_DEFLATED) as z:
            for path in sorted(folder.rglob("*")):
                if path.is_file():
                    z.write(path, path.relative_to(folder.parent))
    else:
        (folder / binary).chmod(0o755)
        artifact = out / (name + ".tar.gz")
        with tarfile.open(artifact, "w:gz") as t:
            t.add(folder, arcname=name)
    digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
    (out / (artifact.name + ".sha256")).write_text(f"{digest}  {artifact.name}\n", encoding="ascii")
    print(f"Packaged {artifact.name}")
