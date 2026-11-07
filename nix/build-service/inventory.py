#!/usr/bin/env python3
"""Inventory only owned NativeLink deployment roots, never the whole Docker store.

Physical-root discovery requires running containers: their mount namespaces are
the authority for exact overlay snapshot paths. Cache-entry discovery explicitly
pauses owned persistent backends during each scan and resumes them on return/error.
Short-lived probes have no network, writable host mounts, or daemon socket.
Image blobs are traversed from the exact OCI descriptors, deduplicated by digest.
Ordinary inventory reports shared candidates separately. Physical inventory closes
the selected file-data extent union, charging exact shared control files in full;
it is a live interval, not a filesystem-metadata or hard total-state bound.

physical-inventory requires an explicit immutable --compsize, repeated --allow-root
and positional roots. --scope active adds the owned deployment, retained Nix
closures, referenced image/snapshot payloads, observer and exact shared controls.
--scope immutable measures only the explicitly supplied payload roots. Its stdout
is one sekka.physical-inventory.v1 JSON line followed by one actual compsize result.
Unsupported layouts, missing retained objects, old builder-created images and
changing object membership fail closed; no boolean-only completeness receipt.
"""
import argparse
from contextlib import ExitStack
from importlib.machinery import SourceFileLoader
from importlib.util import module_from_spec, spec_from_loader
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import uuid

sys.dont_write_bytecode = True


def _bind_script(code, **bindings):
    # Bind data once; never rescan inserted literals as template placeholders.
    return "".join(f"{name} = {value!r}\n" for name, value in bindings.items()) + code


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
    return snapshot_paths(result["root_mountinfo"], container["Id"], snapshot_root)


def snapshot_paths(mount, identifier, snapshot_root):
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
                          "exclusive": key != "lowerdir", "container_id": identifier})
    return roots, mount


def image_blobs(controller, manifest, content_root, images, *, strict=False, execute=None):
    seeds = {image["Descriptor"]["digest"]: image["Descriptor"] for image in images}
    platforms = {(image["Os"], image["Architecture"], image.get("Variant", "")) for image in images}
    code = _bind_script("""import hashlib, json, pathlib, re
root = pathlib.Path(CONTENT)
platforms = PLATFORMS
strict = STRICT
seeds = SEEDS
pending = [(descriptor, False) for descriptor in seeds.values()]
documents = {
    'application/vnd.oci.image.index.v1+json',
    'application/vnd.oci.image.manifest.v1+json',
    'application/vnd.docker.distribution.manifest.list.v2+json',
    'application/vnd.docker.distribution.manifest.v2+json',
}
seen, present, absent = {}, set(), []
while pending:
    descriptor, foreign = pending.pop()
    digest = descriptor['digest']
    if digest in seen and (not seen[digest] or foreign):
        continue
    seen[digest] = foreign
    if not re.fullmatch(r'sha256:[0-9a-f]{64}', digest):
        raise RuntimeError('Unsupported OCI digest ' + digest)
    path = root / digest.split(':', 1)[1]
    if strict and path.is_symlink():
        raise RuntimeError('Symlinked OCI object ' + digest)
    if not path.is_file():
        if digest in seeds or (strict and not foreign):
            raise RuntimeError('Missing required retained OCI object ' + digest)
        absent.append({'digest': digest, 'reason': 'proven foreign-platform descriptor graph',
                       'platform': foreign, 'descriptor': descriptor} if strict else digest)
        continue
    present.add(digest)
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
    encoded = path.read_bytes()
    if strict and 'sha256:' + hashlib.sha256(encoded).hexdigest() != digest:
        raise RuntimeError('OCI descriptor digest mismatch ' + digest)
    document = json.loads(encoded)
    children = document.get('manifests', [])
    foreign_platforms = {}
    for child in children:
        platform = child.get('platform', {})
        identity = (platform.get('os'), platform.get('architecture'))
        if (identity[0] not in (None, 'unknown') and identity[1] not in (None, 'unknown')
                and identity not in {value[:2] for value in platforms}):
            foreign_platforms[child['digest']] = platform
    for child in children:
        annotations = child.get('annotations', {})
        associated = (annotations.get('vnd.docker.reference.digest')
                      if annotations.get('vnd.docker.reference.type') == 'attestation-manifest' else None)
        # An unknown/unknown attestation is optional only when its explicit
        # subject is an incompatible platform manifest in this same index.
        excluded_platform = foreign_platforms.get(child['digest']) or foreign_platforms.get(associated)
        pending.append((child, foreign or excluded_platform or False))
    pending.extend((child, foreign) for child in document.get('layers', []))
    if 'config' in document:
        pending.append((document['config'], foreign))
print(json.dumps({'digests': sorted(present),
                  'absent_descriptors': sorted(absent, key=lambda value: value['digest'] if strict else value)}))
""", SEEDS=seeds, CONTENT=content_root if execute else "/content", PLATFORMS=platforms, STRICT=strict)
    result = execute(code) if execute else probe(controller, manifest, code, mounts=[(content_root, "/content")])
    roots = [{"path": content_root + "/" + digest.split(":", 1)[1],
              "category": "image_content_blob", "exclusive": False, "digest": digest}
             for digest in result["digests"]]
    return roots, result["absent_descriptors"]


def identities(controller, manifest, roots):
    paths = sorted({root["path"] for root in roots})
    mounts = [(path, "/roots/" + str(index)) for index, path in enumerate(paths)]
    code = _bind_script("""import json, os, stat
paths = PATHS
result = []
for index, path in enumerate(paths):
    status = os.stat('/roots/' + str(index), follow_symlinks=False)
    result.append({'path': path, 'device': str(os.major(status.st_dev)) + ':' + str(os.minor(status.st_dev)),
                   'inode': status.st_ino, 'mode': stat.S_IMODE(status.st_mode),
                   'kind': 'directory' if stat.S_ISDIR(status.st_mode) else 'file',
                   'allocated_bytes_at_root_inode': status.st_blocks * 512})
print(json.dumps(result))
""", PATHS=paths)
    return probe(controller, manifest, code, mounts=mounts, read_search=True)


def cache_entries(controller, manifest, timeout):
    if manifest.get("ephemeral_cache"):
        raise SystemExit("Cache-entry inventory requires persistent deployment storage")
    roots = [manifest["directory"] + "/" + domain + "/data/" + store
             for domain in sorted(manifest["domains"]) for store in ("cas", "ac")]
    limits = [manifest[store + "_bytes"]
              for domain in sorted(manifest["domains"]) for store in ("cas", "ac")]
    entry_limits = [controller.cache_entry_limit(limit) for limit in limits]
    scan = """import json, os, stat, sys, time
from pathlib import Path
started = time.time()
roots = [Path(value) for value in json.loads(sys.argv[1])]
block_size = int(sys.argv[2])
files, errors, sizes, counts, accounted_sizes = {}, [], [], [], []
for index, root in enumerate(roots):
    if root.resolve(strict=True) != root:
        raise RuntimeError('Refusing symlinked cache root: ' + str(root))
    if not root.is_dir():
        raise RuntimeError('Cache root is not a directory: ' + str(root))
    size, count, accounted_size = 0, 0, 0
    for directory, _, names in os.walk(root, followlinks=False, onerror=lambda error: errors.append(str(error))):
        for name in names:
            path = Path(directory) / name
            try:
                info = path.lstat()
                if stat.S_ISREG(info.st_mode):
                    relative = path.relative_to(root)
                    files[str(index) + '/' + str(relative)] = info.st_size
                    size += info.st_size
                    accounted_size += ((info.st_size + block_size - 1) // block_size) * block_size
                    count += relative.parts[0] == 'content'
            except OSError as error:
                errors.append(str(error))
    sizes.append(size)
    counts.append(count)
    accounted_sizes.append(accounted_size)
print(json.dumps({'schema': 1, 'roots': list(map(str, roots)), 'files': files,
                  'file_count': len(files), 'apparent_bytes': sum(sizes),
                  'root_apparent_bytes': sizes, 'root_entry_counts': counts, 'errors': errors,
                  'root_block_accounted_bytes': accounted_sizes,
                  'start_unix': started, 'end_unix': time.time()}))
"""
    deadline = time.monotonic() + timeout
    observations = []
    while True:
        backends = []
        for domain in sorted(manifest["domains"]):
            backend = controller.inspect_owned(manifest, controller.name(manifest, domain, "backend"))
            if not backend or not backend["State"]["Running"] or backend["State"]["Paused"]:
                raise SystemExit("Cache-entry inventory requires running, unpaused owned backends")
            backends.append(backend["Id"])
        with ExitStack() as resumed:
            for backend in backends:
                controller.docker(manifest, "pause", backend, capture=True)
                resumed.callback(controller.docker, manifest, "unpause", backend, capture=True)
            result = json.loads(controller.remote(
                manifest, sys.executable, "-B", "-c", scan, json.dumps(roots),
                str(controller.CACHE_BLOCK_SIZE), capture=True))
        within_limits = (
            all(size <= limit for size, limit in zip(result["root_block_accounted_bytes"], limits))
            and all(count <= limit for count, limit in zip(result["root_entry_counts"], entry_limits)))
        observations.append({key: result[key] for key in (
            "start_unix", "end_unix", "file_count", "root_apparent_bytes",
            "root_block_accounted_bytes", "root_entry_counts", "errors")})
        if result["errors"] or within_limits or time.monotonic() >= deadline:
            if not within_limits:
                result["errors"].append("Persistent cache has not reached its configured byte/entry bounds")
            result.update(
                method="Owned backends paused; apparent and block-rounded policy bytes, NOT physical extent accounting",
                backend_ids=backends, configured_root_byte_limits=limits,
                configured_root_entry_limits=entry_limits, bound_observations=observations)
            print(json.dumps(result, sort_keys=True))
            return not result["errors"]
        # Successful complete snapshots above the new bound are progress
        # observations, not failed scans to retry. Let asynchronous eviction run.
        time.sleep(min(0.25, max(0, deadline - time.monotonic())))


def discover(controller, manifest, *, physical=False):
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
            if container["LogPath"] and not container["LogPath"].startswith(metadata + "/"):
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
    # Physical mode closes blobs and identities inside the final observer rather
    # than allocating extra probes whose identities would already be stale.
    blobs, absent = ([], []) if physical else image_blobs(controller, manifest, content_root, images)
    roots.extend(blobs)
    root_identities = [] if physical else identities(controller, manifest, roots)
    native = json.loads(controller.remote(manifest, sys.executable, "-B", "-c",
        "import json,os,pathlib,sys; print(json.dumps(sorted(os.readlink(p) for p in pathlib.Path(sys.argv[1]).iterdir() if p.is_symlink())))",
        manifest["directory"] + "/roots", capture=True))
    if any(not path.startswith("/nix/store/") or "/" in path[len("/nix/store/"):] for path in native):
        raise SystemExit("Unexpected retained Nix store root")
    requisites = controller.remote(manifest, "nix-store", "--query", "--requisites", *native,
                                  capture=True).splitlines() if native else []
    return {
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
    }


def physical_inventory(controller, manifest, args):
    """Measure a closed, live file-extent root set, not allocator-level storage."""
    started = time.time()
    # Resolve on the deployment host, never against the caller's filesystem.
    preparation_code = """
import json, os, pathlib, shutil, sys
request = json.loads(sys.argv[1])
def canonical(value):
    path = pathlib.Path(value)
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise RuntimeError('Noncanonical or symlinked accounting path: ' + value)
    if not (path.is_dir() or path.is_file()):
        raise RuntimeError('Nonregular accounting root: ' + value)
    return path
allowed = [canonical(value) for value in request['allowed']]
roots = [canonical(value) for value in request['roots']]
for path in roots:
    if not any(path == parent or path.is_relative_to(parent) for parent in allowed):
        raise RuntimeError('Root outside explicit allowlist: ' + str(path))
    if str(path) in ('/', '/nix', '/nix/store', '/nix/var', '/nix/var/nix', '/var', '/var/lib'):
        raise RuntimeError('Broad global accounting root is prohibited: ' + str(path))
compsize = pathlib.Path(request['compsize']).resolve(strict=True)
if not compsize.is_file() or not os.access(compsize, os.X_OK) or not compsize.is_relative_to('/nix/store'):
    raise RuntimeError('compsize must be an executable in the immutable Nix store')
store_roots = set()
links = []
def retain(value):
    path = pathlib.Path(value).resolve(strict=True)
    if path.is_relative_to('/nix/store') and len(path.parts) >= 4:
        store_roots.add('/nix/store/' + path.parts[3])
    else:
        raise RuntimeError('Required executable is not an immutable Nix input: ' + str(path))
if request['active']:
    scan_roots = roots + [canonical(request['deployment'])]
    for root in scan_roots:
        if root.is_relative_to('/nix/store'):
            retain(root)
            continue
        if not root.is_dir():
            continue
        for directory, directories, files in os.walk(root, followlinks=False,
                onerror=lambda error: (_ for _ in ()).throw(error)):
            for name in directories + files:
                path = pathlib.Path(directory) / name
                if path.is_symlink():
                    target = path.resolve()
                    if target.is_relative_to('/nix/store'):
                        retain(str(target))
                        links.append({'path': str(path), 'target': str(target)})
    for value in request['tools'] + [str(compsize)]:
        retain(value)
    for name in ('docker', 'ssh', 'nix-store'):
        value = shutil.which(name)
        if not value:
            raise RuntimeError('Missing inventory executable: ' + name)
        retain(value)
print(json.dumps({'roots': sorted(set(map(str, roots))), 'allowed_roots': list(map(str, allowed)),
                  'compsize': str(compsize), 'nix_roots': sorted(store_roots),
                  'nix_links': sorted(links, key=lambda link: (link['path'], link['target']))}))
"""
    request = {
        "allowed": args.allow_root, "roots": args.roots, "compsize": args.compsize,
        "active": args.scope == "active", "deployment": manifest["directory"],
        "tools": [sys.executable, str(args.controller), str(Path(__file__).resolve())]
                 + os.environ.get("SEKKA_BUILD_SERVICE_INVENTORY_INPUTS", "").split(),
    }
    selected = json.loads(controller.remote(manifest, sys.executable, "-B", "-c", preparation_code,
                                            json.dumps(request), capture=True))
    active = args.scope == "active"
    inventory = discover(controller, manifest, physical=True) if active else None
    engine = json.loads(controller.docker(manifest, "info", "--format", "{{json .}}", capture=True))
    if ["driver-type", "io.containerd.snapshotter.v1"] not in engine["DriverStatus"]:
        raise SystemExit("Unresolved physical scope: unsupported Docker image-store layout")
    docker_root = engine["DockerRootDir"].rstrip("/")
    daemon_root = docker_root + "/containerd/daemon"
    snapshotter = daemon_root + "/io.containerd.snapshotter.v1.overlayfs"
    content_root = daemon_root + "/io.containerd.content.v1.content/blobs/sha256"
    namespace = engine.get("Containerd", {}).get("Namespaces", {}).get("Containers")
    if active and (engine["Driver"] != "overlayfs" or not namespace
                   or not re.fullmatch(r"[A-Za-z0-9_.-]+", namespace)):
        raise SystemExit("Unresolved containerd runtime namespace or snapshotter")
    runtime_root = daemon_root + "/io.containerd.runtime.v2.task/" + (namespace or "")
    for root in selected["roots"]:
        if Path(docker_root).is_relative_to(root) or Path(root).is_relative_to(docker_root):
            raise SystemExit("Explicit payload roots cannot sweep or override attributed Docker roots")
        if Path("/nix/var/nix").is_relative_to(root) or Path(root).is_relative_to("/nix/var/nix"):
            raise SystemExit("Explicit payload roots cannot sweep shared Nix control state")
    metadata_layout = {
        docker_root + "/image": {"required": ["identity-cache.db"], "optional": []},
        docker_root + "/network/files": {"required": ["local-kv.db"], "optional": []},
        daemon_root + "/io.containerd.metadata.v1.bolt": {"required": ["meta.db"], "optional": []},
        daemon_root + "/io.containerd.grpc.v1.introspection": {"required": ["uuid"], "optional": []},
        snapshotter: {"required": ["metadata.db"], "optional": [], "directories": ["snapshots"]},
        "/nix/var/nix/db": {"required": ["db.sqlite", "schema"],
                          "optional": ["db.sqlite-wal", "db.sqlite-shm", "db.sqlite-journal",
                                       "big-lock", "reserved"]},
        "/nix/var/nix/temproots": {"required": [], "optional": [], "file_pattern": r"[0-9]+"},
        "/nix/var/nix/userpool": {"required": [], "optional": [], "file_pattern": r"[0-9]+"},
    }
    metadata_leaves = [docker_root + "/engine-id", "/nix/var/nix/gc.lock"]
    observer_image = json.loads(controller.docker(
        manifest, "image", "inspect", controller.NGINX, capture=True))[0]
    observer_image_id = observer_image["Id"]
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", observer_image_id):
        raise SystemExit("Unresolved observer image identity")
    roots = [{"path": root, "category": "explicit_owned_state" if active else "immutable_payload",
              "exclusive": active} for root in selected["roots"]]
    nix_inputs = {"roots": [], "requisites": [], "gc_root_links": []}
    if active:
        roots.extend(inventory["root_provenance"])
        native = sorted(set(selected["nix_roots"] + inventory["shared_readonly_native_inputs"]["roots"]))
        requisites = controller.remote(manifest, "nix-store", "--query", "--requisites", *native,
                                       capture=True).splitlines()
        if not requisites or any(not re.fullmatch(r"/nix/store/[0-9a-z]{32}-[^/]+", p) for p in requisites):
            raise SystemExit("Unresolved retained Nix closure")
        # A store path can itself be a symlink (e.g. a man output pointing at
        # another output). It has no file data extents, and its target is in the
        # closure, so it is recorded but never passed to the observer as a root.
        symlinks = set(controller.remote(manifest, "find", *sorted(set(requisites)), "-maxdepth", "0",
                                         "-type", "l", capture=True).splitlines())
        nix_inputs = {"roots": native, "requisites": sorted(set(requisites)),
                      "symlink_requisites": sorted(symlinks), "gc_root_links": selected["nix_links"]}
        roots.extend({"path": p, "category": "retained_nix_closure", "exclusive": False}
                     for p in nix_inputs["requisites"] if p not in symlinks)
    # Parent mounts bootstrap the observer's own ID/snapshot, not measurement
    # roots. Never enumerate foreign containers/snapshots or measure these parents.
    mounts = {"/nix/store", *selected["roots"]}
    if active:
        mounts.update({manifest["directory"], docker_root + "/containers", content_root,
                       runtime_root, *metadata_layout, *metadata_leaves, "/nix/var/nix/gcroots/auto"})
    mounts = sorted(path for path in mounts
                    if not any(path != other and Path(path).is_relative_to(other) for other in mounts))
    # Source-parent views only restat exact objects. Control parents disable
    # submount copying; never walk their foreign children or count mount aliases.
    parents = {str(Path(path).parent) for path in selected["roots"]}
    if active:
        parents.update({docker_root, "/nix/var/nix", str(Path(manifest["directory"]).parent)})
    source_views = {path: "/__physical_sources/" + str(index)
                    for index, path in enumerate(sorted(parents))}
    source_recursion = {
        path: ("disabled" if any(Path(control).is_relative_to(path)
                                 for control in (docker_root, "/nix/var/nix")) else "readonly")
        for path in source_views}
    rootfs_views = ({container["id"]: "/__physical_rootfs/" + container["id"]
                     for container in inventory["containers"]} if active else {})
    token = uuid.uuid4().hex
    observer_name = manifest["name"] + "-physical-" + token
    label = controller.LABEL + ".physical-observer"
    argv = ["create", "--name", observer_name, "--pull=never", "--network=none", "--read-only",
            "--cap-drop=ALL", "--cap-add=SYS_ADMIN", "--cap-add=DAC_READ_SEARCH",
            "--security-opt=no-new-privileges", "--user=0:0", "--log-driver=none",
            "--memory=512m", "--cpus=1", "--pids-limit=64", "--label", label + "=" + token,
            "--label", controller.LABEL + ".physical-deployment=" + manifest["name"]]
    def mount_arg(source, target, recursive):
        # Docker refuses rprivate bind sources under its daemon root. A read-only
        # rslave bind receives daemon mount changes without propagating back.
        # Docker forbids combining rslave with recursively read-only mounts, so
        # bind exact daemon objects without child mounts and reject any nested
        # source mountpoints before measuring them.
        under_daemon = Path(source).is_relative_to(docker_root)
        propagation = "rslave" if under_daemon else "rprivate"
        if under_daemon:
            recursive = "disabled"
        return (f"type=bind,source={source},target={target},readonly,"
                f"bind-propagation={propagation},bind-recursive={recursive}")
    for path in mounts:
        if "," in path:
            raise SystemExit("Unsupported comma in physical observer mount")
        argv += ["--mount", mount_arg(path, path, "readonly")]
    for source, target in source_views.items():
        if "," in source:
            raise SystemExit("Unsupported comma in source-parent mount")
        argv += ["--mount", mount_arg(source, target, source_recursion[source])]
    for identifier, target in rootfs_views.items():
        source = docker_root + "/rootfs/overlayfs/" + identifier
        argv += ["--mount", mount_arg(source, target, "disabled")]
    argv += ["--entrypoint", sys.executable, observer_image_id,
             "-B", "-c", "import time; time.sleep(86400)"]
    observer_id = None

    def inspect_observer():
        info = json.loads(controller.docker(manifest, "inspect", observer_name, capture=True))[0]
        if (info["Config"]["Labels"].get(label) != token or info["Image"] != observer_image_id
                or (observer_id is not None and info["Id"] != observer_id)):
            raise RuntimeError("Refusing foreign or replaced physical observer")
        return info

    def execute(code):
        return json.loads(controller.docker(manifest, "exec", "-i", observer_id,
                                            sys.executable, "-B", "-", capture=True, input=code))

    def membership():
        ids = controller.docker(manifest, "ps", "-a", "--no-trunc", "--filter",
                                "label=" + controller.LABEL + "=" + manifest["name"],
                                "--format", "{{.ID}}", capture=True).splitlines()
        infos = json.loads(controller.docker(manifest, "inspect", *sorted(ids), capture=True)) if ids else []
        return sorted((info["Id"], info["Image"], info["State"]["Running"],
                       info["State"]["Pid"], sorted(
                           (mount["Type"], mount.get("Source", ""), mount["Destination"], mount.get("RW", False))
                           for mount in info["Mounts"])) for info in infos)

    def auxiliary_membership():
        for suffix, expected in ((".inventory", []), (".physical-deployment", [observer_id])):
            retained = sorted(controller.docker(
                manifest, "ps", "-a", "--no-trunc", "--filter",
                "label=" + controller.LABEL + suffix + "=" + manifest["name"],
                "--format", "{{.ID}}", capture=True).splitlines())
            if retained != expected:
                raise RuntimeError("Unresolved retained accounting observer/probe: " + suffix)
        return sorted(controller.docker(
            manifest, "network", "ls", "--no-trunc", "--filter",
            "label=" + controller.LABEL + "=" + manifest["name"],
            "--format", "{{.ID}}", capture=True).splitlines())

    metadata_code = _bind_script("""
import json, pathlib, re, stat
layout = LAYOUT
result = []
for directory, policy in layout.items():
    parent = pathlib.Path(directory)
    if parent.resolve(strict=True) != parent:
        raise RuntimeError('Symlinked shared metadata parent: ' + directory)
    found = {path.name: path for path in parent.iterdir()}
    permitted = set(policy['required'] + policy['optional'] + policy.get('directories', []))
    if 'file_pattern' in policy:
        permitted.update(name for name in found if re.fullmatch(policy['file_pattern'], name))
    if set(found) - permitted or set(policy['required']) - set(found):
        raise RuntimeError('Unresolved shared metadata layout: ' + directory + ' entries=' + repr(sorted(found)))
    for name, path in sorted(found.items()):
        status = path.lstat()
        if name in policy.get('directories', []):
            if not stat.S_ISDIR(status.st_mode):
                raise RuntimeError('Unexpected snapshot parent type')
            continue
        if not stat.S_ISREG(status.st_mode):
            raise RuntimeError('Nonregular shared control file: ' + str(path))
        result.append({'path': str(path), 'category': 'shared_control_metadata',
                       'exclusive': False, 'charge_class': 'shared_conservative',
                       'device': status.st_dev, 'inode': status.st_ino})
for value in LEAVES:
    path = pathlib.Path(value)
    status = path.lstat()
    if path.resolve(strict=True) != path or not stat.S_ISREG(status.st_mode):
        raise RuntimeError('Nonregular shared control leaf: ' + value)
    result.append({'path': value, 'category': 'shared_control_metadata',
                   'exclusive': False, 'charge_class': 'shared_conservative',
                   'device': status.st_dev, 'inode': status.st_ino})
print(json.dumps({'files': result, 'absent_optional': [
    str(pathlib.Path(directory) / name) for directory, policy in layout.items()
    for name in policy['optional'] if not (pathlib.Path(directory) / name).exists()]}))
""", LAYOUT=metadata_layout, LEAVES=metadata_leaves)
    try:
        daemon_sources = [path for path in mounts if Path(path).is_relative_to(docker_root)]
        def check_daemon_mounts():
            code = _bind_script("""import json, pathlib, re
roots = [pathlib.Path(value) for value in ROOTS]
mounts = []
for line in pathlib.Path('/proc/self/mountinfo').read_text().splitlines():
    encoded = line.split()[4]
    value = re.sub(re.escape(chr(92)) + r'([0-7]{3})', lambda m: chr(int(m[1], 8)), encoded)
    path = pathlib.Path(value)
    if any(path != root and path.is_relative_to(root) for root in roots):
        mounts.append(value)
print(json.dumps(sorted(set(mounts))))
""", ROOTS=daemon_sources)
            nested = json.loads(controller.remote(manifest, sys.executable, "-B", "-c", code, capture=True))
            if nested:
                raise RuntimeError("Unresolved nested Docker daemon mounts beneath measured roots: " + repr(nested))
            return nested
        daemon_mounts_before = check_daemon_mounts()
        # Resolve cleanup by unpredictable exact name even if create returns
        # malformed output after successfully allocating an owned container.
        observer_id = controller.docker(manifest, *argv, capture=True).strip()
        if not re.fullmatch(r"[0-9a-f]{64}", observer_id):
            observer_id = None
            raise RuntimeError("Invalid created observer identity")
        inspect_observer()
        controller.docker(manifest, "start", observer_id, capture=True)
        observer = inspect_observer()
        if not observer["State"]["Running"]:
            raise RuntimeError("Physical observer failed to start")
        images = []
        absent = []
        shared = {"files": [], "absent_optional": []}
        construction = None
        runtime_evidence = {}
        before_membership = membership() if active else []
        if active:
            before_networks = auxiliary_membership()
            if before_networks != sorted(network["id"] for network in inventory["frontend_networks"]):
                raise RuntimeError("Unresolved retained owned networks")
            if {item[0] for item in before_membership} != {item["id"] for item in inventory["containers"]}:
                raise RuntimeError("Unresolved retained owned containers outside discovered roles")
            mount = execute("import json\nprint(json.dumps(next(line.strip() for line in open('/proc/1/mountinfo') if line.split()[4] == '/')))")
            snapshots, _ = snapshot_paths(mount, observer_id, snapshotter + "/snapshots")
            roots.extend(snapshots)
            roots.append({"path": docker_root + "/containers/" + observer_id,
                          "category": "observer_metadata", "exclusive": True, "container_id": observer_id})
            # Moby's snapshotter mounter uses rootfs/<snapshotter>/<container ID>.
            # Compare each exact mounted alias to that container's actual
            # namespace mountinfo; never scan the merged overlay as new payload.
            rootfs_mounts = execute(_bind_script("""
import json, pathlib
targets = TARGETS
lines = pathlib.Path('/proc/self/mountinfo').read_text().splitlines()
print(json.dumps({identifier: next(line for line in lines if line.split()[4] == target)
                  for identifier, target in targets.items()}))
""", TARGETS=rootfs_views))
            for container in inventory["containers"]:
                alias_roots, _ = snapshot_paths(rootfs_mounts[container["id"]], container["id"],
                                               snapshotter + "/snapshots")
                original_roots, _ = snapshot_paths(container["root_mountinfo"], container["id"],
                                                  snapshotter + "/snapshots")
                if {root["path"] for root in alias_roots} != {root["path"] for root in original_roots}:
                    raise RuntimeError("Owned rootfs alias differs from its actual mount namespace")
            runtime_code = _bind_script("""
import json, pathlib, stat
identifiers = IDENTIFIERS
runtime_root = pathlib.Path(RUNTIME)
source_root = pathlib.Path(SOURCE)
result = {'runtime_roots': [], 'rootfs_backing_directories': []}
for identifier in identifiers:
    runtime = runtime_root / identifier
    status = runtime.lstat()
    if runtime.resolve(strict=True) != runtime or not stat.S_ISDIR(status.st_mode):
        raise RuntimeError('Unresolved owned runtime metadata directory: ' + str(runtime))
    result['runtime_roots'].append({'path': str(runtime), 'category': 'containerd_runtime_metadata',
        'exclusive': True, 'container_id': identifier, 'device': status.st_dev, 'inode': status.st_ino})
    backing = source_root / 'rootfs/overlayfs' / identifier
    status = backing.lstat()
    if (backing.resolve(strict=True) != backing or not stat.S_ISDIR(status.st_mode)
            or any(backing.iterdir())):
        raise RuntimeError('Unresolved retained payload beneath owned rootfs mountpoint: ' + identifier)
    result['rootfs_backing_directories'].append({'container_id': identifier,
        'device': status.st_dev, 'inode': status.st_ino, 'entries': []})
print(json.dumps(result))
""", IDENTIFIERS=sorted([observer_id, *(item[0] for item in before_membership)]),
                RUNTIME=runtime_root, SOURCE=source_views[docker_root])
            runtime_before = execute(runtime_code)
            runtime_evidence = dict(runtime_before, rootfs_alias_mountinfo=rootfs_mounts)
            roots.extend(runtime_evidence["runtime_roots"])
            references = sorted({manifest["image"], controller.NGINX, observer_image_id,
                                 *(item[1] for item in before_membership)})
            images = json.loads(controller.docker(manifest, "image", "inspect", *references, capture=True))
            image_references = dict(zip(references, [image["Id"] for image in images]))
            images = list({image["Id"]: image for image in images}.values())
            covered = selected["roots"] + [manifest["directory"]] + nix_inputs["requisites"]
            for identifier, image_id, running, pid, binds in before_membership:
                if not running:
                    raise RuntimeError("Retained owned container stopped before measurement: " + identifier)
                for kind, source, destination, writable in binds:
                    if kind == "tmpfs" or (kind == "bind" and source == "/nix/store" and not writable):
                        continue
                    if kind != "bind" or not any(Path(source).is_relative_to(root) for root in covered):
                        raise RuntimeError("Unresolved retained container mount: " + source)
            blobs, absent = image_blobs(controller, manifest, content_root, images, strict=True, execute=execute)
            roots = [root for root in roots if root["category"] != "image_content_blob"] + blobs
            shared = execute(metadata_code)
            roots.extend(shared["files"])
            construction = execute(_bind_script("""
import hashlib, json, pathlib, tarfile
directory = pathlib.Path(DIRECTORY)
receipt = directory / 'image/construction.json'
if receipt.resolve(strict=True) != receipt or receipt.stat().st_size > 1024 * 1024:
    raise RuntimeError('Missing or invalid owned image import receipt')
proof = json.loads(receipt.read_text())
if proof.get('schema') != 1 or proof.get('method') != 'docker-image-import-v1':
    raise RuntimeError('Unresolved builder payload attribution: image was not attested rootfs import')
rootfs = proof['rootfs']
if rootfs['path'] != 'image/rootfs.tar':
    raise RuntimeError('Unexpected owned rootfs archive path')
archive = directory / rootfs['path']
if archive.resolve(strict=True) != archive:
    raise RuntimeError('Symlinked rootfs archive')
with archive.open('rb') as stream:
    digest = hashlib.file_digest(stream, 'sha256').hexdigest()
if digest != rootfs['sha256'] or rootfs['diff_id'] != 'sha256:' + digest:
    raise RuntimeError('Imported rootfs archive/diffID mismatch')
with tarfile.open(archive, 'r:') as stream:
    members = stream.getmembers()
    if ([member.name.rstrip('/') for member in members] != ['bin', 'bin/nativelink']
            or not members[0].isdir() or not members[1].isfile()
            or any((m.mode, m.uid, m.gid, m.mtime) != (0o755, 0, 0, 0) for m in members)):
        raise RuntimeError('Unexpected imported rootfs members')
    with stream.extractfile(members[1]) as binary:
        if hashlib.file_digest(binary, 'sha256').hexdigest() != proof['source']['binary_sha256']:
            raise RuntimeError('Imported executable hash mismatch')
print(json.dumps(proof))
""", DIRECTORY=manifest["directory"]))
            imported = construction["image"]
            expected = next((image for image in images if image["Id"] == imported["id"]), None)
            if (construction["source"]["version"] != controller.VERSION
                    or construction["source"]["archive_sha256"] != controller.ARCHIVE_SHA256
                    or image_references[manifest["image"]] != imported["id"]
                    or image_references[controller.NGINX] != observer_image_id):
                raise RuntimeError("Image import proof differs from pinned deployment inputs")
            if (expected is None or imported["os"] != "linux" or imported["architecture"] != "amd64"
                    or imported["entrypoint"] != ["/bin/nativelink"] or imported["rootfs_type"] != "layers"
                    or imported["diff_ids"] != [construction["rootfs"]["diff_id"]]
                    or expected["Os"] != imported["os"] or expected["Architecture"] != imported["architecture"]
                    or expected["Config"]["Entrypoint"] != imported["entrypoint"]
                    or expected["RootFS"] != {"Type": imported["rootfs_type"], "Layers": imported["diff_ids"]}
                    or any(image["Id"] not in (imported["id"], observer_image_id) for image in images)):
                raise RuntimeError("Unresolved retained image/import proof or unexpected image membership")
            gc_links = execute(_bind_script("""
import json, pathlib
owned = pathlib.Path(DIRECTORY) / 'roots'
result = []
for path in pathlib.Path('/nix/var/nix/gcroots/auto').iterdir():
    if path.is_symlink():
        target = pathlib.Path(path.readlink())
        if not target.is_absolute():
            target = path.parent / target
        if target.is_relative_to(owned):
            result.append({'path': str(path), 'target': str(target), 'inode': path.lstat().st_ino})
print(json.dumps(result))
""", DIRECTORY=manifest["directory"]))
            nix_inputs["indirect_gc_root_links"] = gc_links
        # Some roots also live in the before/after membership snapshots. Keep
        # those snapshots immutable while enriching the public provenance.
        roots = [{**root, "charge_class": root.get(
            "charge_class", "exclusive" if root["exclusive"] else "shared_referenced")}
                 for root in roots]
        paths = sorted({root["path"] for root in roots})
        provenance = {
            "schema": 1, "protocol": "sekka.physical-inventory.v1", "scope": args.scope,
            "method": "one compression-aware referenced file-data extent union",
            "roots": paths, "root_provenance": roots, "allowed_roots": selected["allowed_roots"],
            "explicit_roots": selected["roots"], "compsize": selected["compsize"],
            "observer": {"container_id": observer_id, "image_id": observer_image_id,
                         "descriptor": observer_image["Descriptor"], "root_mountinfo": mount if active else None,
                         "included_in_union": active, "log_driver": "none", "read_only": True,
                         "read_only_parent_mounts": mounts, "source_restat_views": source_views,
                         "source_restat_recursion": source_recursion,
                         "rootfs_alias_views": rootfs_views},
            "images": [{"id": image["Id"], "descriptor": image["Descriptor"]} for image in images],
            "absent_oci_descriptors": absent, "shared_metadata": shared,
            "image_references": image_references if active else {},
            "nix_inputs": nix_inputs, "image_construction": construction,
            "container_membership": before_membership,
            "runtime_evidence": runtime_evidence,
            "frontend_networks": inventory["frontend_networks"] if active else [],
            "discovery_start_unix": started, "unresolved": [],
            "shared_metadata_layout": metadata_layout if active else {},
            "docker_engine": {"server_version": engine["ServerVersion"], "driver": engine["Driver"],
                              "driver_status": engine["DriverStatus"], "root": docker_root},
            "scope_exclusions": {
                "builder_cache": ("attested docker-image-import-v1 archive, inspected rootfs diffID and image identity"
                                  if active else "outside explicit immutable payload scope"),
                "global_host_state": "host journals, allocator metadata and unrelated daemon/service state are not deployment file extents",
                "other_nix_profiles": "only retained deployment/supplied roots and actual inventory tool closures participate",
                "nix_build_scratch": "already-realized pinned/retained closures; this inventory only queries nix-store and never realizes derivations",
            },
            "limitations": [
                "Live non-atomic interval; concurrent writes can change extents during scanning.",
                "Shared control files are conservatively charged in full, including foreign records and slack.",
                "File data extents only: excludes global Btrfs trees, inode/directory allocation, reservations and RAM/tmpfs.",
                "Running observer is the cutoff; teardown and subsequent controller/daemon writes occur afterward.",
                "No hard total-state bound or exclusive attribution of shared metadata is asserted.",
                "Immutable scope contains explicit payload roots only, excluding observer/tool/shared-control overhead.",
                "GC symlink identities are recorded, not treated as independently additive file extents.",
            ],
        }
        result = execute(_bind_script("""
import fcntl, json, os, pathlib, re, stat, subprocess, time
paths = PATHS
source_views = SOURCE_VIEWS
def identities():
    result = {}
    nested_mounts = set()
    for line in pathlib.Path('/proc/self/mountinfo').read_text().splitlines():
        mount = re.sub(re.escape(chr(92)) + r'([0-7]{3})', lambda match: chr(int(match[1], 8)), line.split()[4])
        if any(mount != root and pathlib.Path(mount).is_relative_to(root) for root in paths):
            nested_mounts.add(mount)
    for value in sorted(set(paths) | nested_mounts):
        path = pathlib.Path(value)
        if path.resolve(strict=True) != path:
            raise RuntimeError('Root replaced with a symlink: ' + value)
        fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
        try:
            status = os.fstat(fd)
            for parent, target in source_views.items():
                if path.is_relative_to(parent) and str(path) != parent:
                    source = pathlib.Path(target) / path.relative_to(parent)
                    source_status = source.lstat()
                    if (source.resolve(strict=True) != source
                            or (source_status.st_dev, source_status.st_ino, source_status.st_mode)
                            != (status.st_dev, status.st_ino, status.st_mode)):
                        raise RuntimeError('Host source identity differs from bound root: ' + value)
            if not (stat.S_ISDIR(status.st_mode) or stat.S_ISREG(status.st_mode)):
                raise RuntimeError('Nonregular extent root: ' + value)
            info = bytearray(1024)
            fcntl.ioctl(fd, 0x8400941f, info, True)
            result[value] = {'btrfs_uuid': bytes(info[16:32]).hex(), 'device': status.st_dev,
                             'inode': status.st_ino, 'mode': status.st_mode}
        finally:
            os.close(fd)
    if len({entry['btrfs_uuid'] for entry in result.values()}) != 1:
        raise RuntimeError('Union spans Btrfs filesystem UUIDs')
    return {'roots': {path: result[path] for path in paths},
            'nested_mounts': {path: result[path] for path in sorted(nested_mounts)}}
before = identities()
start = time.time()
result = subprocess.run([COMPSIZE, '-b', '--', *paths], capture_output=True, text=True)
end = time.time()
after = identities()
if before != after:
    raise RuntimeError('Extent root identity changed during measurement')
if result.returncode:
    raise RuntimeError('compsize failed: ' + result.stderr)
if sum(line.split()[:1] == ['TOTAL'] for line in result.stdout.splitlines()) != 1:
    raise RuntimeError('compsize did not emit exactly one TOTAL row')
print(json.dumps({'filesystem_validation': {'method': 'BTRFS_IOC_FS_INFO', **before},
                  'measurement_start_unix': start, 'measurement_end_unix': end,
                  'stdout': result.stdout, 'stderr': result.stderr, 'exit_status': result.returncode}))
""", PATHS=paths, COMPSIZE=selected["compsize"], SOURCE_VIEWS=source_views))
        inspect_observer()
        if active:
            if before_membership != membership():
                raise RuntimeError("Owned container membership changed during measurement")
            if shared != execute(metadata_code):
                raise RuntimeError("Shared control-file membership changed during measurement")
            if before_networks != auxiliary_membership():
                raise RuntimeError("Owned network or observer membership changed during measurement")
            runtime_after = execute(runtime_code)
            if runtime_before != runtime_after:
                raise RuntimeError("Owned runtime metadata membership changed during measurement")
            if daemon_mounts_before != check_daemon_mounts():
                raise RuntimeError("Nested Docker daemon mounts changed during measurement")
            final_images = json.loads(controller.docker(manifest, "image", "inspect", *references, capture=True))
            if dict(zip(references, [image["Id"] for image in final_images])) != image_references or {
                    (image["Id"], json.dumps(image["Descriptor"], sort_keys=True)) for image in final_images} != {
                    (image["Id"], json.dumps(image["Descriptor"], sort_keys=True)) for image in images}:
                raise RuntimeError("Retained image membership changed during measurement")
        after_inputs = json.loads(controller.remote(manifest, sys.executable, "-B", "-c", preparation_code,
                                                     json.dumps(request), capture=True))
        if after_inputs != selected:
            raise RuntimeError("Explicit roots, tool identities or retained Nix inputs changed during measurement")
        provenance.update({key: value for key, value in result.items() if key not in ("stdout", "stderr")})
        provenance["cutoff"] = "measurement_end_unix, observer still running; cleanup excluded"
        print(json.dumps(provenance, sort_keys=True), flush=True)
        sys.stdout.write(result["stdout"])
        sys.stderr.write(result["stderr"])
    finally:
        # Never use label-wide removal. Ownership and immutable image are checked
        # again before removing this one ID, including on discovery failure.
        found = controller.docker(manifest, "ps", "-a", "--filter", "name=^/" + observer_name + "$",
                                  "--format", "{{.ID}}", capture=True).strip()
        if found:
            owned = inspect_observer()
            controller.docker(manifest, "rm", "-f", owned["Id"], capture=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--controller", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--local", action="store_true")
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--cache-entries", action="store_true")
    modes.add_argument("--physical", action="store_true")
    parser.add_argument("--timeout", type=int, default=60)
    parser.add_argument("--scope", choices=("active", "immutable"))
    parser.add_argument("--compsize")
    parser.add_argument("--allow-root", action="append", default=[])
    parser.add_argument("roots", nargs="*")
    args = parser.parse_args()
    if args.timeout < 0:
        parser.error("--timeout must be nonnegative")
    if args.physical and not (args.scope and args.compsize and args.allow_root and args.roots):
        parser.error("--physical requires --scope, --compsize, --allow-root and explicit roots")
    if not args.physical and (args.scope or args.compsize or args.allow_root or args.roots):
        parser.error("Physical root options require --physical")
    if args.manifest.stat().st_mode & 0o077:
        raise SystemExit("Credential manifest must be mode0600")
    loader = SourceFileLoader("service_inventory_controller", str(args.controller))
    controller = module_from_spec(spec_from_loader(loader.name, loader))
    loader.exec_module(controller)
    controller.LOCAL = args.local
    manifest = json.loads(args.manifest.read_text())
    controller.assert_owned_directory(manifest)
    if args.cache_entries:
        raise SystemExit(0 if cache_entries(controller, manifest, args.timeout) else 1)
    if args.physical:
        physical_inventory(controller, manifest, args)
    else:
        print(json.dumps(discover(controller, manifest), indent=2))


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        if error.stderr:
            print(error.stderr, file=sys.stderr)
        raise
