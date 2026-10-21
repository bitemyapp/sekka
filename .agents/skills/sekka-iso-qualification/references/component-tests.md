# Component fixtures and comparison evidence

Read this before running a component VM or comparing original and fixed builds.
These checks can establish a defect and its correction; they do not complete an
ISO gate. Keep the original candidate and its failure evidence unchanged.

## Isolate each driver run

Give every run its own short, private `XDG_RUNTIME_DIR`, `TMPDIR`, working
directory and output directory, including sequential original/fixed runs. The
pinned NixOS test driver chooses `XDG_RUNTIME_DIR` before `TMPDIR` for VM state
and VLAN sockets. Setting only `TMPDIR` can therefore reuse another driver's
`vm-state-machine`, shared directory or VLAN socket under `/run/user/UID`.
Check the current driver's implementation if its version changes.

Use mode 0700 directories owned by the invoking user. Keep the runtime root
short, such as `/tmp/sk-vm.XXXXXX`: Linux pathname UNIX sockets have a 108-byte
address field, including the terminator. Leave room for the driver's appended
machine and socket names; a long evidence-directory path can exceed the limit.
Keep durable logs outside the temporary runtime directory.

For an absolute `driver` store path and an existing absolute `evidence_root`,
this launches one isolated run and preserves its exit status:

```sh
umask 077
run_dir=$(mktemp -d "$evidence_root/component.XXXXXX")
vm_runtime=$(mktemp -d /tmp/sk-vm.XXXXXX)
mkdir -m 700 "$vm_runtime/tmp" "$run_dir/work" "$run_dir/output"
printf '%s\n' "$vm_runtime" > "$run_dir/runtime-path.txt"
if (
  cd "$run_dir/work" || exit
  export XDG_RUNTIME_DIR="$vm_runtime" TMPDIR="$vm_runtime/tmp"
  "$driver/bin/nixos-test-driver" --no-interactive -o "$run_dir/output"
) > "$run_dir/driver.log" 2>&1; then
  driver_status=0
else
  driver_status=$?
fi
printf '%s\n' "$driver_status" > "$run_dir/exit-status.txt"
```

Record the driver derivation and invocation as well. Before removing runtime
state, verify that this run's driver, QEMU and helper processes have exited and
copy any needed disks, journals or sockets' diagnostic metadata into evidence.
Do not remove another run's sockets or kill its processes to clear a collision.
Allocate separate disk/firmware files and loopback ports where the fixture
requires them; coordinate memory and CPU use with other running VMs.

## Bind the comparison to the code that ran

For direct Cargo builds, use distinct `CARGO_TARGET_DIR` directories for the
original and fixed source trees. If reusing a target is necessary, record an
explicit clean limited to the affected workspace/package and rebuild it before
testing. Switching a checkout or copying source alone is not proof that the
launched binary changed. Avoid broad cleaning of shared caches.

Record each side's commit, lock hash, local diff, build command/environment,
package and test-driver derivations, and relevant filtered source hashes. Verify
the executable actually launched in the guest, including wrappers, runtime
environment, assets and dependencies relevant to the behavior. A source-tree
hash alone does not establish which package was loaded.

List fixture differences from production explicitly: autologin, alternate
display/session setup, injected modules, synthetic devices, test data, patched
packages and extra diagnostic tools. Preserve production packaging where the
claim depends on it. For example, a whole-root script test cannot prove that its
helper and runtime dependencies are present in an initrd; inspect the generated
archive and exercise the actual boot path. A fixture bypassing the greeter
cannot validate normal login.

Use the same final input/assertion script for original and fixed controls where
possible. If their scripts or environments differ, retain both and explain why
the difference does not remove the failing precondition. Bind visible UI input,
inspected original-resolution screenshots and backend assertions to that run.

## Preserve failures and review boundaries

Retain failed harness attempts, commands, exit statuses and raw logs. Distinguish
a product failure from a bad fixture, missing tool, timing/discovery race or
driver collision using evidence. Record each harness correction and its scope;
do not silently replace the failed attempt with a passing retry. Freeze copies
of mutable input/diagnostic logs before hashing a completed evidence manifest.
Keep credentials and private fixture keys out of public evidence.

At commit, rebase or merge boundaries, record old and new revisions, relevant
diffs, lock hashes and evaluated package/test-driver derivations. If the tested
product, fixture and driver are identical, retain that equivalence audit with
the original tested-head manifest; a documentation-only or unrelated change
does not require an unchanged component VM to run again. If they differ,
identify the affected behavior and rerun the checks needed to validate it.
Do not rewrite the historical manifest to make an untested head look tested.

Component success, package equivalence and a reviewed fix provide no automatic
ISO credit. Follow [Q7](failures.md): build a clean new candidate and repeat the
required gates on its exact hash.
