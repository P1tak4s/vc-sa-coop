"""Build the player's package: dist/VC-SA-Coop/ and dist/VC-SA-Coop-<version>.zip

    python -I tools/make_release.py

The zip holds VC-SA-Coop.exe (install, settings, play), game/ (files for the game folder) and extra/ (repak.exe,
MenuPatch.exe, ReShade). Nothing is packed inside the exe: it copies plain files, so it does not look like a dropper.
extra/ is taken from dist/SA-Coop/extra of an earlier build (third-party tools, not in git).
"""
import os, re, shutil, subprocess, sys, zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CARGO = os.path.join(os.path.expanduser("~"), ".cargo", "bin", "cargo.exe")
OLD_EXTRA = os.path.join(ROOT, "dist", "SA-Coop", "extra")
OUT = os.path.join(ROOT, "dist", "VC-SA-Coop")

subprocess.run([CARGO, "build", "--release", "-p", "coopnet", "-p", "sa-coop"], cwd=ROOT, check=True)
rel = os.path.join(ROOT, "target", "release")
version = re.search(r'^version = "([^"]+)"', open(os.path.join(ROOT, "launcher", "Cargo.toml"), encoding="utf-8").read(), re.M).group(1)
shutil.copy(os.path.join(rel, "coopnet64.dll"), os.path.join(ROOT, "game", "coopnet64.asi"))
shutil.rmtree(OUT, ignore_errors=True)
shutil.copytree(os.path.join(ROOT, "game"), os.path.join(OUT, "game"))
shutil.copy(os.path.join(rel, "VC-SA-Coop.exe"), OUT)
os.makedirs(os.path.join(OUT, "extra"))
for name in ("repak.exe", "MenuPatch.exe", "THIRD_PARTY.txt"):
    shutil.copy(os.path.join(OLD_EXTRA, name), os.path.join(OUT, "extra"))
shutil.copytree(os.path.join(OLD_EXTRA, "reshade"), os.path.join(OUT, "extra", "reshade"))
shutil.copy(os.path.join(ROOT, "docs", "README.txt"), OUT)
zpath = os.path.join(ROOT, "dist", f"VC-SA-Coop-{version}-windows-x64.zip")
with zipfile.ZipFile(zpath, "w", zipfile.ZIP_DEFLATED) as z:
    for base, _, files in os.walk(OUT):
        for f in files:
            full = os.path.join(base, f)
            z.write(full, os.path.join("VC-SA-Coop", os.path.relpath(full, OUT)))
print(zpath, os.path.getsize(zpath) // 1024, "KB")
