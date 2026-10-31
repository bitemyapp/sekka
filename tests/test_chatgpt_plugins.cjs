// Exercise the packaged app's actual executor-copy function against store modes.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const os = require("node:os");
const vm = require("node:vm");
const { createRequire } = require("node:module");

async function main() {
  const resources = path.resolve(process.argv[2]);
  const archive = await fs.readFile(path.join(resources, "app.asar"));
  const header = JSON.parse(archive.subarray(16, 16 + archive.readUInt32LE(12)));
  const entries = header.files[".vite"].files.build.files;
  // New releases move the executor into an early-bootstrap chunk. Find the
  // actual function, requiring one definition across the packaged JS files.
  const definitions = [];
  for (const [name, entry] of Object.entries(entries)) {
    if (!name.endsWith(".js") || entry.offset === undefined) continue;
    const start = 8 + archive.readUInt32LE(4) + Number(entry.offset);
    const bytes = archive.subarray(start, start + entry.size);
    if (!bytes.includes(Buffer.from("executorPluginRoot:"))) continue;
    const code = bytes.toString();
    for (const match of code.matchAll(/async function [\w$]+\(\{executorPluginRoot:[\w$]+,resourcesPath:[\w$]+\}\).*?(?=async function )/gs)) {
      definitions.push({ name, code, copy: match[0] });
    }
  }
  assert.equal(definitions.length, 1, "exactly one executor copy function must be present");
  const { name, code, copy } = definitions[0];
  // Compile the entire modified file, then call the real function in isolation.
  new vm.Script(code);
  const configName = copy.match(/await ([\w$]+)\(\{useWsl:!1,resourcesPath:/)[1];
  const fsBinding = copy.match(/([\w$]+)(\.default|\(\))\.writeFile/);
  const pathBinding = copy.match(/([\w$]+)(\.default|\(\))\.join/);
  assert(fsBinding && pathBinding, "executor filesystem and path bindings must be recognized");
  const binding = (match, value) => match[2] === "()" ? () => value : { default: value };
  const hostName = copy.match(/CODEX_APP_TOOLS_CALLER_HOST_ID:([\w$]+)\./)[1];
  const hostProperty = copy.match(/CODEX_APP_TOOLS_CALLER_HOST_ID:[\w$]+\.([\w$]+)/)[1];
  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), "chatgpt-plugins-"));
  const source = path.join(temporary, "source");
  const destination = path.join(temporary, "executor-plugins", "codex-app-tools");
  const external = path.join(temporary, "external");
  const command = path.join(source, "scripts", "launch_codex_app_tools_mcp");
  const packagedRequire = createRequire(path.join(resources, "app.asar", ".vite", "build", name));
  const copyPlugin = packagedRequire("../../../sekka-copy-plugin.cjs");
  const context = {
    require: packagedRequire,
    process,
    [configName]: async () => ({ cwd: source, command, env: { KEPT: "yes" } }),
    [fsBinding[1]]: binding(fsBinding, fs),
    [pathBinding[1]]: binding(pathBinding, path),
    [hostName]: { [hostProperty]: "local" },
  };
  const run = new vm.Script(`(${copy})`).runInNewContext(context);
  try {
    await fs.mkdir(path.dirname(command), { recursive: true });
    await fs.writeFile(command, "#!/bin/sh\nexit 0\n", { mode: 0o555 });
    await fs.writeFile(path.join(source, ".mcp.json"), "{}", { mode: 0o444 });
    await fs.writeFile(external, "keep read-only", { mode: 0o444 });
    await fs.symlink(external, path.join(source, "linked-file"));
    await fs.chmod(path.dirname(command), 0o555);
    await fs.chmod(source, 0o555);

    // Negative control: the unpatched copy cannot rewrite its own manifest.
    const control = path.join(temporary, "unpatched");
    await fs.cp(source, control, { recursive: true });
    await assert.rejects(fs.writeFile(path.join(control, ".mcp.json"), "updated"), { code: "EACCES" });

    for (let attempt = 0; attempt < 3; attempt++) {
      if (attempt === 1) {
        // Upgrade from the already-broken package, including read-only parents.
        await fs.chmod(path.join(destination, "scripts"), 0o555);
        await fs.chmod(path.join(destination, ".mcp.json"), 0o444);
        await fs.chmod(destination, 0o555);
      }
      assert.equal(await run({ executorPluginRoot: destination, resourcesPath: resources }), destination);
      const manifest = JSON.parse(await fs.readFile(path.join(destination, ".mcp.json")));
      assert.equal(manifest.mcpServers.codex_app.cwd, destination);
      assert.equal(manifest.mcpServers.codex_app.command, path.join(destination, "scripts", path.basename(command)));
      assert.equal(manifest.mcpServers.codex_app.enabled, true);
      assert.equal(manifest.mcpServers.codex_app.env.KEPT, "yes");
      assert.equal(manifest.mcpServers.codex_app.env.CODEX_APP_TOOLS_CALLER_HOST_ID, "local");
      assert((await fs.stat(path.join(destination, "scripts", path.basename(command)))).mode & 0o100);
      assert.equal((await fs.stat(external)).mode & 0o777, 0o444);
    }
    await Promise.all(Array.from({ length: 12 }, () => run({ executorPluginRoot: destination, resourcesPath: resources })));
    assert.equal(JSON.parse(await fs.readFile(path.join(destination, ".mcp.json"))).mcpServers.codex_app.cwd, destination);
    const linkedDestination = path.join(temporary, "linked-destination");
    await fs.symlink(destination, linkedDestination);
    await assert.rejects(copyPlugin(source, linkedDestination), /not a directory/);
    console.log("Packaged executor: fresh copy, repeat and concurrent refresh, damaged copy, command and manifest, symlink isolation passed; unpatched control fails.");
  } finally {
    // Make only real fixture entries writable before removing read-only copies.
    async function cleanup(entry) {
      const stat = await fs.lstat(entry);
      if (stat.isSymbolicLink()) return;
      await fs.chmod(entry, stat.mode | 0o700);
      if (stat.isDirectory()) for (const name of await fs.readdir(entry)) await cleanup(path.join(entry, name));
    }
    await cleanup(temporary);
    await fs.rm(temporary, { recursive: true, force: true });
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
