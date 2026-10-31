#!/usr/bin/env python3
"""Bounded controls for the source-only soak observer."""
import importlib.util
from pathlib import Path
import unittest
import json
import tempfile

spec = importlib.util.spec_from_file_location("soak", Path(__file__).with_name("soak.py"))
soak = importlib.util.module_from_spec(spec)
spec.loader.exec_module(soak)
rlib_spec = importlib.util.spec_from_file_location("rlib", Path(__file__).with_name("soak-rlib.py"))
rlib = importlib.util.module_from_spec(rlib_spec)
rlib_spec.loader.exec_module(rlib)


class GrowthFlags(unittest.TestCase):
    def flagged(self, blocks):
        rows = [dict(age_s=300+60*i, rss_kib=value)
                for i, value in enumerate(v for b in blocks for v in [b]*5)]
        return soak.flags(rows, "rss_kib", 32768)

    def test_late_monotonic_growth_after_an_early_decrease(self):
        self.assertTrue(self.flagged([120000, 90000, 100000, 140000]))

    def test_flat_or_decreasing_or_small_growth(self):
        for blocks in [[90000]*4, [140000, 100000, 90000], [90000, 100000, 110000]]:
            with self.subTest(blocks=blocks):
                self.assertFalse(self.flagged(blocks))

    def test_needs_three_consecutive_nondecreasing_blocks(self):
        self.assertFalse(self.flagged([90000, 140000, 80000, 130000]))

    def test_warmup_does_not_flag(self):
        rows = [dict(age_s=i, rss_kib=i*100000) for i in range(20)]
        self.assertFalse(soak.flags(rows, "rss_kib", 32768))


class RlibSelection(unittest.TestCase):
    def test_exact_cli_manifest_selects_among_feature_variants(self):
        with tempfile.TemporaryDirectory() as directory:
            deps = Path(directory)
            (deps/"libtokio-other.rlib").write_bytes(b"other profile")
            selected = deps/"libtokio-cli.rlib"
            selected.write_bytes(b"CLI profile")
            manifest = deps/"cli.json"
            manifest.write_text(json.dumps({"tokio": str(selected)}))
            self.assertEqual(rlib.select_rlib(deps, "tokio", manifest), selected.resolve())

    def test_single_variant_needs_no_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            selected = Path(directory)/"libtokio-cli.rlib"
            selected.write_bytes(b"CLI profile")
            self.assertEqual(rlib.select_rlib(directory, "tokio"), selected.resolve())

    def test_changed_cli_artifact_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            deps = Path(directory)
            selected = deps/"libtokio-cli.rlib"
            selected.write_bytes(b"changed artifact")
            manifest = deps/"cli.json"
            manifest.write_text(json.dumps({"tokio": str(selected), "_sha256": {"tokio": "0"*64}}))
            with self.assertRaises(AssertionError):
                rlib.select_rlib(deps, "tokio", manifest)


if __name__ == "__main__":
    unittest.main()
