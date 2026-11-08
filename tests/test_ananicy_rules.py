"""Record-aware filtering of Ananicy rules, without changing other content."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "filter_rules", Path(__file__).resolve().parents[1] / "nix/scheduling/filter-rules.py"
)
rules = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rules)


class RuleFilter(unittest.TestCase):
    def test_filters_decoded_name_independent_of_json_spelling(self):
        source = ('{ "name" : "bash", "nice": -4 }\n'
                  '{"nice": -4, "na\\u006de": "fi\\u0073h"}\n'
                  '{"name":"bash","description":"Unicode separator: \u2028"}\n'
                  '{"name":"bash-helper","description":"bash"}\n')
        self.assertEqual(rules.filtered(source, {"bash", "fish"}), '{"name":"bash-helper","description":"bash"}\n')

    def test_preserves_comments_other_rules_and_malformed_upstream_lines(self):
        source = ('# {"name":"bash"}\r\n\r\n'
                  '{"name":"foot","metadata":{"name":"bash"}}\r\n'
                  '{"name":["bash"]}\n'
                  '{"name":"Dungeon Drafters.x86_64","type":"Game"}+\n')
        self.assertEqual(rules.filtered(source, {"bash"}), source)
        self.assertEqual(rules.filtered(rules.filtered(source, {"bash"}), {"bash"}), source)


if __name__ == "__main__":
    unittest.main()
