"""The offline graph check skips compilation bodies, not declared build inputs."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("generate", Path(__file__).with_name("generate.py"))
generate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(generate)


class GraphInputTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        for name in ("desktop/aizome/Cargo.toml", "desktop/aizome/Cargo.lock", "MODULE.bazel",
                     "build/bazel/features.json", "build/bazel/generate.py", "crate/Cargo.toml", "crate/src/lib.rs"):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("")
        for name, value in {"ROOT": self.root, "FEATURES_FILE": self.root / "build/bazel/features.json"}.items():
            replacement = patch.object(generate, name, value)
            replacement.start()
            self.addCleanup(replacement.stop)

    def inputs(self):
        return generate.graph_inputs(["crate"])

    def test_rust_body_edit_is_left_to_bazel_source_digest(self):
        source = self.root / "crate/src/lib.rs"
        source.write_text("pub fn value() -> u8 { 1 }\n")
        before = self.inputs()
        source.write_text("pub fn value() -> u8 { 2 }\n")
        self.assertEqual(before, self.inputs())

    def test_manifest_and_new_target_change_graph(self):
        before = self.inputs()
        (self.root / "crate/Cargo.toml").write_text('[dependencies]\nserde = "1"\n')
        self.assertNotEqual(before, self.inputs())
        before = self.inputs()
        (self.root / "crate/src/main.rs").write_text("fn main() {}\n")
        self.assertNotEqual(before, self.inputs())

    def test_include_reference_and_referent_existence_change_graph(self):
        source = self.root / "crate/src/lib.rs"
        before = self.inputs()
        source.write_text('const DATA: &str = include_str!("data.txt");\n')
        self.assertNotEqual(before, self.inputs())
        before = self.inputs()
        (source.parent / "data.txt").write_text("present")
        self.assertNotEqual(before, self.inputs())
        before = self.inputs()
        (source.parent / "data.txt").write_text("changed content")
        self.assertEqual(before, self.inputs())  # Bazel hashes declared compile_data.

    def test_unrelated_buck_metadata_does_not_change_graph(self):
        before = self.inputs()
        (self.root / "crate/BUCK").write_text("new unrelated metadata")
        self.assertEqual(before, self.inputs())


if __name__ == "__main__":
    unittest.main()
