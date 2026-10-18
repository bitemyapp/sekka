// fs.cp preserves Nix store modes. Repair an earlier copy before replacing it,
// and the new copy before the app writes .mcp.json. Never chmod symlink targets.
const fs = require("node:fs/promises");
const path = require("node:path");
const pending = new Map();

async function writable(entry) {
  const stat = await fs.lstat(entry).catch((error) => {
    if (error.code === "ENOENT") return null;
    throw error;
  });
  if (stat === null || stat.isSymbolicLink()) return;
  if (stat.uid !== process.getuid()) {
    throw new Error(`Plugin copy is not owned by this user: ${entry}`);
  }
  await fs.chmod(entry, stat.mode | 0o200);
  if (stat.isDirectory()) {
    for (const name of await fs.readdir(entry)) {
      await writable(path.join(entry, name));
    }
  }
}

async function copyPlugin(source, destination) {
  const existing = await fs.lstat(destination).catch((error) => {
    if (error.code === "ENOENT") return null;
    throw error;
  });
  if (existing !== null && !existing.isDirectory()) {
    throw new Error(`Plugin destination is not a directory: ${destination}`);
  }
  await writable(destination);
  await fs.cp(source, destination, {
    recursive: true,
    // The caller generates this file immediately after copying. Copying the
    // store's manifest would make it read-only during another caller's write.
    filter: (entry) => entry !== path.join(source, ".mcp.json"),
  });
  await writable(destination);
}

module.exports = function copyPluginSerially(source, destination) {
  // Startup and capability refresh can overlap. Do not let one fs.cp restore
  // store modes while another copy is writing the same destination.
  const previous = pending.get(destination) ?? Promise.resolve();
  const operation = previous.catch(() => {}).then(() => copyPlugin(source, destination));
  pending.set(destination, operation);
  return operation.finally(() => {
    if (pending.get(destination) === operation) pending.delete(destination);
  });
};
