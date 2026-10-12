"""Build plugins and zip them for release.

    python bundle.py --list              # show the plugins you can bundle
    python bundle.py vepres              # build, then zip
    python bundle.py vepres sanctis      # several plugins, one build
    python bundle.py vepres --no-build   # zip what's already in target/bundled
    python bundle.py --all [--no-build]  # every plugin in bundler.toml, one build
    python bundle.py vepres --fast       # build with --profile fast (quick compile, not for release)
    python bundle.py vepres --copy       # also copy the .vst3 to the system VST3 folder
    python bundle.py vepres --copy PATH  # ... or to PATH

Output: dist/<NAME>-<VERSION>-win64.zip (and the unzipped folder next to it).

Per plugin it expects:
  - a [<plugin>] entry with `name` in bundler.toml
  - a workspace package named <plugin> (version is read from its Cargo.toml)
  - README.md next to that Cargo.toml (README_*.md translations are optional)
  - optional: presets/<NAME>/ in the repo root (e.g. presets/C-SANCTIS/). It goes
    into the zip as Presets/, for the user's Documents/Anselic/Sacria/<NAME>/Presets folder.
"""

import argparse
import shutil
import subprocess
import sys
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
EXTRA_FILES = [ROOT / "assets" / "fonts" / "GeistMono-OFL.txt"]
VST3_DIR = "C:/Program Files/Common Files/VST3"


def die(msg):
    sys.exit(f"error: {msg}")


def load_toml(path):
    with open(path, "rb") as f:
        return tomllib.load(f)


def find_package(plugin):
    """Return the crate dir of the workspace member whose package name is `plugin`."""
    for pattern in load_toml(ROOT / "Cargo.toml")["workspace"]["members"]:
        for manifest in sorted(ROOT.glob(f"{pattern}/Cargo.toml")):
            if load_toml(manifest)["package"]["name"] == plugin:
                return manifest.parent
    die(f"no workspace package named '{plugin}'")


def check(plugin, bundler):
    """Fail early (before the build) if `plugin` cannot be zipped. Returns its crate dir."""
    if plugin not in bundler:
        die(f"'{plugin}' not in bundler.toml")
    crate_dir = find_package(plugin)
    if not (crate_dir / "README.md").is_file():
        die(f"missing {(crate_dir / 'README.md').relative_to(ROOT)}")
    return crate_dir


def build(plugins, fast):
    """One cargo call for all of them: cargo builds them in parallel and shares
    the dependencies. If any plugin fails to compile, nothing gets zipped."""
    profile = ["--profile", "fast"] if fast else ["--release"]
    cmd = ["cargo", "xtask", "bundle", *(a for p in plugins for a in ("-p", p)), *profile]
    if subprocess.run(cmd, cwd=ROOT).returncode != 0:
        die("build failed, nothing zipped")


def zip_release(name, crate_dir):
    version = load_toml(crate_dir / "Cargo.toml")["package"]["version"]
    if isinstance(version, dict):  # version.workspace = true
        version = load_toml(ROOT / "Cargo.toml")["workspace"]["package"]["version"]
    readme = crate_dir / "README.md"

    bundled = ROOT / "target" / "bundled"
    plugins = [bundled / f"{name}.vst3", bundled / f"{name}.clap"]
    for p in plugins:
        if not p.exists():
            die(f"missing {p.relative_to(ROOT)} (build first?)")

    release = f"{name}-{version}"
    out_dir = ROOT / "dist" / release
    shutil.rmtree(out_dir, ignore_errors=True)
    out_dir.mkdir(parents=True)

    for p in plugins:
        if p.is_dir():
            shutil.copytree(p, out_dir / p.name)
        else:
            shutil.copy2(p, out_dir)
    # translated READMEs (README_TH.md, ...) go in too when present
    for f in [readme, *sorted(crate_dir.glob("README_*.md")), *EXTRA_FILES]:
        shutil.copy2(f, out_dir)
    # presets that ship with the plugin: plain files, the plugin reads them from the user's folder
    if (ROOT / "presets" / name).is_dir():
        shutil.copytree(ROOT / "presets" / name, out_dir / "Presets")

    zip_path = ROOT / "dist" / f"{release}-win64.zip"
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as z:
        for f in sorted(out_dir.rglob("*")):
            z.write(f, f.relative_to(out_dir))

    print(f"wrote {zip_path.relative_to(ROOT)}")


def install(name, dest):
    """Copy the built .vst3 into `dest`, over any copy already there."""
    src = ROOT / "target" / "bundled" / f"{name}.vst3"
    try:
        shutil.copytree(src, dest / src.name, dirs_exist_ok=True)
    except OSError as e:
        die(f"could not copy {src.name} to {dest}: {e}\n"
            "(Program Files needs an admin terminal; a running DAW can lock the file)")
    print(f"copied {src.name} -> {dest}")


def main():
    ap = argparse.ArgumentParser(description="Build and zip plugins.")
    ap.add_argument("plugin", nargs="*", help="package name(s), e.g. vepres sanctis")
    ap.add_argument("--all", action="store_true", help="bundle every plugin in bundler.toml")
    ap.add_argument("--list", action="store_true", help="show the plugins in bundler.toml and exit")
    ap.add_argument("--no-build", action="store_true", help="skip cargo xtask bundle")
    ap.add_argument("--fast", action="store_true",
                    help="build with --profile fast (no LTO, quicker compile) instead of --release")
    ap.add_argument("--copy", nargs="?", const=VST3_DIR, metavar="PATH",
                    help=f"also copy the .vst3 to PATH (default: {VST3_DIR})")
    args = ap.parse_args()

    bundler = load_toml(ROOT / "bundler.toml")
    # `--copy vepres`: argparse takes the plugin name as PATH, give it back
    if args.copy and args.copy.lower() in bundler:
        args.plugin.insert(0, args.copy)
        args.copy = VST3_DIR
    if args.list:
        for plugin, entry in bundler.items():
            print(f"{plugin:<16} {entry['name']}")
        return
    if args.all == bool(args.plugin):
        ap.error("give plugin name(s) or --all")
    if args.fast and args.no_build:
        ap.error("--fast does nothing with --no-build")

    # dict.fromkeys: drop repeats, keep the order given
    plugins = list(bundler) if args.all else list(dict.fromkeys(p.lower() for p in args.plugin))
    crate_dirs = [check(p, bundler) for p in plugins]
    if not args.no_build:
        build(plugins, args.fast)
    for plugin, crate_dir in zip(plugins, crate_dirs):
        zip_release(bundler[plugin]["name"], crate_dir)
    if args.copy:
        for plugin in plugins:
            install(bundler[plugin]["name"], Path(args.copy))


if __name__ == "__main__":
    main()
