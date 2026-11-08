"""Build VC-SA-Coop-Setup.exe: one file with the whole mod inside.

    python -I tools/make_setup.py

Needs dist/SA-Coop/extra (repak.exe, MenuPatch.exe, reshade/) from an earlier release; rebuilds everything else.
"""
import os, shutil, subprocess, sys, zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DIST = os.path.join(ROOT, "dist", "SA-Coop")
CARGO = os.path.join(os.path.expanduser("~"), ".cargo", "bin", "cargo.exe")


def cargo(*args, env=None):
    subprocess.run([CARGO, *args], cwd=ROOT, check=True, env=env)


cargo("build", "--release", "-p", "coopnet", "-p", "sa-coop")
rel = os.path.join(ROOT, "target", "release")
shutil.copy(os.path.join(rel, "coopnet64.dll"), os.path.join(ROOT, "game", "coopnet64.asi"))
# the player's package: launcher + game files + extras (no test tools)
shutil.rmtree(os.path.join(DIST, "game"), ignore_errors=True)
shutil.copytree(os.path.join(ROOT, "game"), os.path.join(DIST, "game"))
shutil.copy(os.path.join(rel, "sa-coop.exe"), DIST)
for need in ("repak.exe", "MenuPatch.exe", os.path.join("reshade", "ReShade64.dll")):
    if not os.path.isfile(os.path.join(DIST, "extra", need)):
        sys.exit(f"missing dist/SA-Coop/extra/{need}")
payload = os.path.join(ROOT, "target", "setup-payload.zip")
with zipfile.ZipFile(payload, "w", zipfile.ZIP_DEFLATED) as z:
    for base, _, files in os.walk(DIST):
        rel_base = os.path.relpath(base, DIST)
        if rel_base.split(os.sep)[0] == "tools":
            continue
        for f in files:
            z.write(os.path.join(base, f), os.path.normpath(os.path.join(rel_base, f)))
env = dict(os.environ, SETUP_PAYLOAD=payload)
cargo("build", "--release", "-p", "vc-sa-coop-setup", env=env)
out = os.path.join(ROOT, "dist", "VC-SA-Coop-Setup.exe")
shutil.copy(os.path.join(rel, "VC-SA-Coop-Setup.exe"), out)
print(out, os.path.getsize(out) // 1024, "KB")
