#!/usr/bin/env python3
"""Inventory only owned NativeLink deployment roots, never the whole Docker store.

Existing containers must be running: their mount namespaces are the authority
for exact overlay snapshot paths. No containers are started/restarted here.
Short-lived probes have no network, writable host mounts, or daemon socket.
Image blobs are traversed from the exact OCI descriptors, deduplicated by digest.
Shared engine/network/build-cache metadata is reported outside exclusive roots.
"""
import argparse
from importlib.machinery import SourceFileLoader
from importlib.util import module_from_spec, spec_from_loader
import json
from pathlib import Path
import re
import subprocess
import sys
import time

sys.dont_write_bytecode = True


def probe(controller, manifest, code, *, user="0:0", pid=None, mounts=(), read_search=False):
    args = ["run", "--rm", "--pull=never", "-i", "--network", "none", "--read-only", "--cap-drop=ALL",
            "--security-opt=no-new-privileges", "--user", user, "--memory", "256m",
            "--cpus", "1", "--pids-limit", "64", "--log-driver", "none",
            "--label", controller.LABEL + ".inventory=" + manifest["name"],
            "--mount", "type=bind,source=/nix/store,target=/nix/store,readonly"]
    if pid:
        args += ["--pid", "container:" + pid]
    if read_search:
        args += ["--cap-add", "DAC_READ_SEARCH"]
    for source, target in mounts:
        if "," in source or "," in target:
            raise SystemExit("Unsupported comma in inventory mount path")
        args += ["--mount", f"type=bind,source={source},target={target},readonly"]
    args += ["--entrypoint", sys.executable, controller.NGINX, "-B", "-"]
    return json.loads(controller.docker(manifest, *args, capture=True, input=code))


def overlay_paths(controller, manifest, container, snapshot_root):
    # Same numeric UID as PID1 permits mountinfo inspection without ptrace or
    # host PID access. This helper joins only the selected owned PID namespace.
    result = probe(controller, manifest, """import json
line = next(line for line in open('/proc/1/mountinfo') if line.split()[4] == '/')
print(json.dumps({'root_mountinfo': line.strip()}))
""", user=container["Config"]["User"] or "0:0", pid=container["Id"])
    mount = result["root_mountinfo"]
    filesystem, _, options = mount.split(" - ", 1)[1].split(" ", 2)
    if filesystem != "overlay":
        raise SystemExit("Inventory requires the observed Docker containerd overlayfs driver")
    options = dict(item.split("=", 1) for item in options.split(",") if "=" in item)
    roots = []
    for key in ("lowerdir", "upperdir", "workdir"):
        if key not in options:
            raise SystemExit("Incomplete overlay mount metadata: " + key)
        for encoded in options[key].split(":"):
            path = re.sub(r"\\([0-7]{3})", lambda match: chr(int(match[1], 8)), encoded)
            if not re.fullmatch(re.escape(snapshot_root) + r"/[0-9]+/(fs|work)", path):
                raise SystemExit("Refusing unexpected snapshot path " + path)
            roots.append({"path": path, "category": "snapshot_" + key,
                          "exclusive": key != "lowerdir", "container_id": container["Id"]})
    return roots, mount


def image_blobs(controller, manifest, content_root, images):
    seeds = {image["Descriptor"]["digest"]: image["Descriptor"] for image in images}
    code = """import json, pathlib, re
root = pathlib.Path('/content')
seeds = SEEDS
pending = list(seeds.values())
documents = {
    'application/vnd.oci.image.index.v1+json',
    'application/vnd.oci.image.manifest.v1+json',
    'application/vnd.docker.distribution.manifest.list.v2+json',
    'application/vnd.docker.distribution.manifest.v2+json',
}
seen, present, absent = set(), [], []
while pending:
    descriptor = pending.pop()
    digest = descriptor['digest']
    if digest in seen:
        continue
    seen.add(digest)
    if not re.fullmatch(r'sha256:[0-9a-f]{64}', digest):
        raise RuntimeError('Unsupported OCI digest ' + digest)
    path = root / digest.split(':', 1)[1]
    if not path.is_file():
        if digest in seeds:
            raise RuntimeError('Missing owned image descriptor ' + digest)
        absent.append(digest)
        continue
    present.append(digest)
    if path.stat().st_size != descriptor['size']:
        raise RuntimeError('Incomplete OCI object ' + digest)
    if descriptor['mediaType'] not in documents:
        if digest in seeds:
            raise RuntimeError('Unsupported image root media type ' + descriptor['mediaType'])
        continue
    # Only index/manifest metadata is read. Layers/config/attestations remain
    # leaves, so inventory does not warm or copy large image payloads.
    if path.stat().st_size > 16 * 1024 * 1024:
        raise RuntimeError('Unreasonably large OCI descriptor ' + digest)
    document = json.loads(path.read_bytes())
    pending.extend(document.get('manifests', []))
    pending.extend(document.get('layers', []))
    if 'config' in document:
        pending.append(document['config'])
print(json.dumps({'digests': sorted(present), 'absent_descriptors': sorted(absent)}))
""".replace("SEEDS", repr(seeds))
    result = probe(controller, manifest, code, mounts=[(content_root, "/content")])
    roots = [{"path": content_root + "/" + digest.split(":", 1)[1],
              "category": "image_content_blob", "exclusive": False, "digest": digest}
             for digest in result["digests"]]
    return roots, result["absent_descriptors"]


def identities(controller, manifest, roots):
    paths = sorted({root["path"] for root in roots})
    mounts = [(path, "/roots/" + str(index)) for index, path in enumerate(paths)]
    code = """import json, os, stat
paths = PATHS
result = []
for index, path in enumerate(paths):
    status = os.stat('/roots/' + str(index), follow_symlinks=False)
    result.append({'path': path, 'device': str(os.major(status.st_dev)) + ':' + str(os.minor(status.st_dev)),
                   'inode': status.st_ino, 'mode': stat.S_IMODE(status.st_mode),
                   'kind': 'directory' if stat.S_ISDIR(status.st_mode) else 'file',
                   'allocated_bytes_at_root_inode': status.st_blocks * 512})
print(json.dumps(result))
""".replace("PATHS", repr(paths))
    return probe(controller, manifest, code, mounts=mounts, read_search=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--controller", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--local", action="store_true")
    args = parser.parse_args()
    if args.manifest.stat().st_mode & 0o077:
        raise SystemExit("Credential manifest must be mode0600")
    loader = SourceFileLoader("service_inventory_controller", str(args.controller))
    controller = module_from_spec(spec_from_loader(loader.name, loader))
    loader.exec_module(controller)
    controller.LOCAL = args.local
    manifest = json.loads(args.manifest.read_text())
    controller.assert_owned_directory(manifest)
    engine = json.loads(controller.docker(manifest, "info", "--format", "{{json .}}", capture=True))
    if ["driver-type", "io.containerd.snapshotter.v1"] not in engine["DriverStatus"]:
        raise SystemExit("Inventory requires Docker's containerd image store")
    docker_root = engine["DockerRootDir"].rstrip("/")
    daemon_root = docker_root + "/containerd/daemon"
    snapshot_root = daemon_root + "/io.containerd.snapshotter.v1.overlayfs/snapshots"
    content_root = daemon_root + "/io.containerd.content.v1.content/blobs/sha256"
    roots = [{"path": manifest["directory"], "category": "deployment_state", "exclusive": True}]
    containers, networks = [], []
    references = {manifest["image"], controller.NGINX}
    for domain in manifest["domains"]:
        network = controller.inspect_frontend(manifest, domain)
        if network:
            networks.append({"name": network["Name"], "id": network["Id"],
                             "attached_names": sorted(endpoint["Name"] for endpoint in network["Containers"].values())})
        for role in ("network", "gateway", "backend", "worker"):
            container = controller.inspect_owned(manifest, controller.name(manifest, domain, role))
            if not container:
                continue
            if not container["State"]["Running"]:
                raise SystemExit("Inventory requires existing containers running; capture before stop: " + container["Name"])
            identifier = container["Id"]
            if not re.fullmatch(r"[0-9a-f]{64}", identifier):
                raise SystemExit("Unexpected Docker container identity")
            metadata = docker_root + "/containers/" + identifier
            if not container["LogPath"].startswith(metadata + "/"):
                raise SystemExit("Unexpected owned container log location")
            roots.append({"path": metadata, "category": "container_metadata_logs", "exclusive": True,
                          "container_id": identifier})
            snapshots, mount = overlay_paths(controller, manifest, container, snapshot_root)
            roots.extend(snapshots)
            references.add(container["Image"])
            containers.append({"name": container["Name"], "id": identifier, "image": container["Image"],
                               "log_path": container["LogPath"], "root_mountinfo": mount})
    images = json.loads(controller.docker(manifest, "image", "inspect", *sorted(references), capture=True))
    images = list({image["Id"]: image for image in images}.values())
    blobs, absent = image_blobs(controller, manifest, content_root, images)
    roots.extend(blobs)
    root_identities = identities(controller, manifest, roots)
    native = json.loads(controller.remote(manifest, sys.executable, "-B", "-c",
        "import json,os,pathlib,sys; print(json.dumps(sorted(os.readlink(p) for p in pathlib.Path(sys.argv[1]).iterdir() if p.is_symlink())))",
        manifest["directory"] + "/roots", capture=True))
    if any(not path.startswith("/nix/store/") or "/" in path[len("/nix/store/"):] for path in native):
        raise SystemExit("Unexpected retained Nix store root")
    requisites = controller.remote(manifest, "nix-store", "--query", "--requisites", *native,
                                  capture=True).splitlines() if native else []
    print(json.dumps({
        "schema": 1, "deployment": manifest["name"], "host": manifest["host"], "captured_unix": time.time(),
        "roots": sorted({root["path"] for root in roots}), "root_provenance": roots,
        "root_identities": root_identities, "containers": containers, "frontend_networks": networks,
        "images": [{"id": image["Id"], "descriptor": image["Descriptor"]} for image in images],
        "absent_oci_descriptors": absent,
        "shared_readonly_native_inputs": {"roots": native, "requisites": sorted(set(requisites)),
                                         "included_in_service_roots": False},
        "outside_exclusive_attribution_paths_are_unmeasured_candidates": True,
        "outside_exclusive_attribution": [
            {"path": docker_root + "/network", "reason": "shared network database, including foreign attached jobs"},
            {"path": daemon_root + "/io.containerd.metadata.v1.bolt", "reason": "shared engine metadata"},
            {"path": daemon_root + "/io.containerd.snapshotter.v1.overlayfs/metadata.db", "reason": "shared snapshot metadata"},
            {"path": docker_root + "/buildkit", "reason": "shared builder metadata/cache, not exclusively attributable to a service"},
            {"path": "/nix/var/nix", "reason": "shared Nix database and indirect GC-root metadata"}],
        "limitations": [
            "Roots cover deployment data/build context/config/GC symlinks, owned container metadata/logs, referenced overlay snapshots and present OCI image blobs.",
            "Shared immutable roots must be unioned once, not summed per container or deployment.",
            "Native Nix closures are separately enumerated inputs, not silently charged as writable service state.",
            "Global engine/network/builder/Nix metadata and observer-container bookkeeping are outside exclusive attribution; this is not a claim that they occupy zero space.",
            "Existing containers must remain running during discovery; no automatic starts or service mutations occur.",
            "Identity validation is privileged but read-only; physical extent/compression measurement is the caller's responsibility."]
    }, indent=2))


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        if error.stderr:
            print(error.stderr, file=sys.stderr)
        raise
