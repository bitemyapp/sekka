"""The window's first run, downloading a real model from Hugging Face."""

catalog = json.load(open(CATALOG))
MODELS = "/home/alice/.local/share/tasuke/models"


def have():
    """Bytes downloaded so far, finished files and partial ones."""
    return int(machine.succeed(f"if test -d {MODELS}; then du -sb {MODELS} | cut -f1; else echo 0; fi"))


start_session()

with subtest("first run looks at the computer and recommends a model that fits"):
    user("aizomectl spawn aizome-tasuke")
    _, offer = until(
        "the Download button",
        lambda: find(lambda e: e.get("role") == "button" and (e.get("label") or "").startswith("Download ")),
        timeout=120,
    )
    name = offer["label"][len("Download "):].split(" · ")[0]
    model = next(i for i, m in catalog.items() if m["name"] == name)
    total = sum(f["size"] for f in catalog[model]["files"])
    print(f"first run recommends {model}: {total} bytes in {len(catalog[model]['files'])} files")
    machine.screenshot("first-run")
    click("Download", lambda e: e.get("label") == offer["label"])

with subtest("it downloads, pauses and resumes"):
    until("Pause", lambda: find(button("Pause")))
    until("some of it downloaded", lambda: have() > 300 * 2**20, timeout=600)
    click("Pause", button("Pause"))
    until("Resume", lambda: find(button("Resume")))
    time.sleep(3)
    paused = have()
    time.sleep(10)
    assert have() - paused < 16 * 2**20, f"still downloading while paused: {paused} then {have()}"
    machine.screenshot("paused")
    click("Resume", button("Resume"))

with subtest("it finishes, each file checked against the catalog, and the model is in use"):
    until("Ready", shows("is downloaded and checked"), timeout=3000)
    machine.screenshot("ready")
    for f in catalog[model]["files"]:
        path = f"{MODELS}/{model}/{f['path'].rsplit('/', 1)[-1]}"
        assert int(machine.succeed(f"stat -c %s {path}")) == f["size"], path
        assert machine.succeed(f"sha256sum {path}").split()[0] == f["sha256"], path
    machine.fail(f"ls {MODELS}/{model}/*.partial")
    config = user("cat ~/.config/tasuke/config.toml")
    assert f'id = "{model}"' in config, config
